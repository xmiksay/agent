use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use sea_orm::*;
use tokio::sync::{Mutex, Semaphore};
use tracing::{error, info};
use uuid::Uuid;

use crate::auth::store::AuthStore;
use crate::auth::waiter::AuthWaiter;
use crate::config::Config;
use crate::entity::{task_results, tasks};
use crate::jobs::hub::LiveSessions;
use crate::jobs::lifecycle::{AGENT_FAILED, AGENT_PENDING, TASK_FAILED, TASK_STATES};
use crate::jobs::registry::RunningTasks;
use crate::jobs::runner::run_job;
use crate::jobs::types::{ClaudeOutput, TriggerReason};
use crate::project::ProjectStore;
use crate::provider::ProviderRegistry;
use crate::workspace::Workspace;

pub struct TaskStore {
    db: DatabaseConnection,
    semaphore: Arc<Semaphore>,
    seen_events: Arc<Mutex<HashSet<String>>>,
    config: Config,
    providers: ProviderRegistry,
    project_store: Arc<ProjectStore>,
    workspace: Arc<Workspace>,
    running: RunningTasks,
    hub: LiveSessions,
    auth_store: Arc<AuthStore>,
    auth_waiter: AuthWaiter,
}

impl TaskStore {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: DatabaseConnection,
        config: Config,
        providers: ProviderRegistry,
        project_store: Arc<ProjectStore>,
        workspace: Arc<Workspace>,
        running: RunningTasks,
        hub: LiveSessions,
        auth_store: Arc<AuthStore>,
        auth_waiter: AuthWaiter,
    ) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_jobs)),
            seen_events: Arc::new(Mutex::new(HashSet::new())),
            db,
            config,
            providers,
            project_store,
            workspace,
            running,
            hub,
            auth_store,
            auth_waiter,
        }
    }

    pub fn hub(&self) -> &LiveSessions {
        &self.hub
    }

    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    pub fn running(&self) -> &RunningTasks {
        &self.running
    }

    pub async fn set_pid(&self, task_id: Uuid, pid: Option<u32>) -> Result<()> {
        let mut active: tasks::ActiveModel = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?
            .into();
        active.pid = Set(pid.map(|p| p as i64));
        active.update(&self.db).await?;
        Ok(())
    }

    /// Edit a task. The operator may set `task_state` (the human lifecycle) on
    /// ANY task regardless of its current state. Run-managed input fields
    /// (`branch`, `default_branch`) are only editable while the task hasn't
    /// started yet (`task_state == "pending"`) — once running, the worktree is
    /// already checked out. The durable `agent_state` and other run-managed
    /// fields (timestamps, session_id, pid, pending_message) are never editable.
    /// The resulting branch may never equal the default branch, so the "never run
    /// on default" rule holds.
    pub async fn update_task(&self, task_id: Uuid, edits: TaskEdits) -> Result<()> {
        let task = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;

        if let Some(ts) = edits.task_state.as_deref()
            && !TASK_STATES.contains(&ts)
        {
            bail!(
                "invalid task_state '{ts}'; expected one of {}",
                TASK_STATES.join("|")
            );
        }

        let edits_branch = edits.branch.is_some() || edits.default_branch.is_some();
        if edits_branch && task.task_state != "pending" {
            bail!(
                "can only edit branch fields while the task is pending (task_state: {})",
                task.task_state
            );
        }

        // Effective post-edit values, used for the cross-field branch guard.
        let default_branch = edits
            .default_branch
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&task.default_branch)
            .to_string();
        let branch = match edits.branch.as_deref().map(str::trim) {
            Some(b) => {
                if b.is_empty() {
                    bail!("branch must not be empty");
                }
                b.to_string()
            }
            None => task
                .branch
                .clone()
                .unwrap_or_else(|| default_branch.clone()),
        };
        if branch == default_branch {
            bail!("refusing to set task branch to the default branch '{default_branch}'");
        }

        let new_task_state = edits.task_state.clone();
        let mut active: tasks::ActiveModel = task.into();
        if edits_branch {
            active.branch = Set(Some(branch));
            active.default_branch = Set(default_branch);
        }
        if let Some(ts) = new_task_state {
            active.task_state = Set(ts);
        }
        active.update(&self.db).await?;
        Ok(())
    }

    pub async fn set_session_id(&self, task_id: Uuid, session_id: &str) -> Result<()> {
        let mut active: tasks::ActiveModel = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?
            .into();
        active.session_id = Set(Some(session_id.to_string()));
        active.update(&self.db).await?;
        Ok(())
    }

    pub fn providers(&self) -> &ProviderRegistry {
        &self.providers
    }

    pub fn project_store(&self) -> &Arc<ProjectStore> {
        &self.project_store
    }

    pub fn workspace(&self) -> &Arc<Workspace> {
        &self.workspace
    }

    pub fn is_duplicate(&self, event_id: &str) -> bool {
        if let Ok(seen) = self.seen_events.try_lock() {
            seen.contains(event_id)
        } else {
            false
        }
    }

    pub async fn mark_seen(&self, event_id: &str) -> bool {
        let mut seen = self.seen_events.lock().await;
        seen.insert(event_id.to_string())
    }

    pub async fn confirm_task(self: &Arc<Self>, task_id: Uuid) -> Result<()> {
        let task = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await
            .context("db error")?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;

        // A live agent (warm/idle or actively running) is the "already running"
        // case now that the durable column no longer stores `running`.
        if self.hub.is_warm(task_id).await || self.hub.is_running(task_id) {
            bail!("task is already live");
        }

        // One agent per branch: refuse to start while another task on the same
        // project *and branch* is actively live — that's the real conflict, since
        // both would share one worktree. Different branches each have their own
        // clone and run concurrently. At single-operator scale the live set is
        // small, so check hub liveness over the un-finished tasks on this branch.
        if let (Some(pid), Some(branch)) = (task.project_id, task.branch.clone()) {
            let siblings = tasks::Entity::find()
                .filter(tasks::Column::ProjectId.eq(pid))
                .filter(tasks::Column::Branch.eq(branch.clone()))
                .filter(tasks::Column::FinishedAt.is_null())
                .filter(tasks::Column::Id.ne(task_id))
                .all(&self.db)
                .await
                .context("checking concurrent branch tasks")?;
            if let Some(other) = siblings
                .into_iter()
                .find(|t| self.hub.is_running(t.id) || self.hub.is_warm_sync(t.id))
            {
                bail!(
                    "another task ({}) is already live on branch '{branch}'; \
                     wait for it to finish or kill it first",
                    other.id
                );
            }
        }

        // Queue the task for spawn: durable agent_state → pending (so a derived
        // read shows `pending` until the turn loop marks it running).
        self.set_states(task_id, AGENT_PENDING, &task.task_state)
            .await?;

        let store = Arc::clone(self);
        let semaphore = self.semaphore.clone();

        // The job spawn no longer holds a permit for its whole life — run_job
        // acquires one per *turn*, so an idle (warm) agent occupies no slot.
        // Setup failures here mark the task failed; run_job owns the rest of the
        // lifecycle (state transitions, per-turn results, final finish).
        let join = tokio::spawn(async move {
            // Any setup failure below marks the task failed/failed before run_job
            // ever gets to own the lifecycle.
            let fail = || store.finish_task(task_id, AGENT_FAILED, TASK_FAILED, None);

            let trigger: TriggerReason = match serde_json::from_value(task.trigger_data.clone()) {
                Ok(t) => t,
                Err(e) => {
                    error!(%task_id, error = %e, "failed to deserialize trigger");
                    let _ = fail().await;
                    return;
                }
            };

            let Some(service_id) = task.git_service_id else {
                error!(%task_id, "task has no git_service_id");
                let _ = fail().await;
                return;
            };

            let service = match store.providers.service(service_id).await {
                Some(s) => s,
                None => {
                    error!(%task_id, %service_id, "git_service not loaded");
                    let _ = fail().await;
                    return;
                }
            };

            let provider = match store.providers.require(service_id).await {
                Ok(p) => p,
                Err(e) => {
                    error!(%task_id, error = %e, "provider not configured");
                    let _ = fail().await;
                    return;
                }
            };

            info!(%task_id, "job starting");

            let resume_session_id = task.session_id.clone();
            // Consume pending_message: clear the column before the run so a
            // crash doesn't replay the same message, and pass it as the prompt.
            let prompt_override = task.pending_message.clone();
            if prompt_override.is_some()
                && let Err(e) = store.clear_pending_message(task_id).await
            {
                error!(%task_id, error = %e, "failed to clear pending_message");
            }

            let result = run_job(
                task_id,
                trigger,
                service.clone(),
                task.project_id,
                task.git_url.clone(),
                task.project_path.clone(),
                task.default_branch.clone(),
                task.branch.clone(),
                store.config.clone(),
                provider,
                store.workspace.clone(),
                store.project_store.clone(),
                store.hub.clone(),
                store.clone(),
                store.auth_store.clone(),
                store.auth_waiter.clone(),
                semaphore,
                resume_session_id,
                prompt_override,
            )
            .await;

            store.running.unregister(task_id).await;

            if let Err(e) = result {
                // {e:?} prints the full anyhow chain incl. Context layers.
                let chain = format!("{e:?}");
                error!(%task_id, error = %e, chain = %chain, "job failed");
                let _ = store.save_error_result(task_id, &chain).await;
                let _ = fail().await;
            }
        });

        // Register AFTER spawn — claude takes hundreds of ms minimum to start
        // up, so the operator will always have a window to kill. If the task
        // somehow finishes before this line runs, the abort handle is harmless.
        self.running.register(task_id, join.abort_handle()).await;

        Ok(())
    }

    /// Replace the one-to-one result row (each turn overwrites the prior one).
    pub(crate) async fn replace_result(&self, task_id: Uuid, output: &ClaudeOutput) -> Result<()> {
        task_results::Entity::delete_many()
            .filter(task_results::Column::TaskId.eq(task_id))
            .exec(&self.db)
            .await
            .context("clearing prior turn result")?;
        self.save_result(task_id, output).await
    }

    async fn save_result(&self, task_id: Uuid, output: &ClaudeOutput) -> Result<()> {
        let result = task_results::ActiveModel {
            id: Set(Uuid::new_v4()),
            task_id: Set(task_id),
            cost_usd: Set(output.total_cost_usd),
            input_tokens: Set(output.input_tokens as i64),
            output_tokens: Set(output.output_tokens as i64),
            num_turns: Set(output.num_turns as i32),
            is_error: Set(output.is_error),
            result_text: Set(output.result.clone()),
            session_id: Set(output.session_id.clone()),
        };

        task_results::Entity::insert(result)
            .exec(&self.db)
            .await
            .context("failed to insert task result")?;

        Ok(())
    }

    async fn save_error_result(&self, task_id: Uuid, error: &str) -> Result<()> {
        let result = task_results::ActiveModel {
            id: Set(Uuid::new_v4()),
            task_id: Set(task_id),
            cost_usd: Set(0.0),
            input_tokens: Set(0),
            output_tokens: Set(0),
            num_turns: Set(0),
            is_error: Set(true),
            result_text: Set(error.to_string()),
            session_id: Set(String::new()),
        };

        task_results::Entity::insert(result)
            .exec(&self.db)
            .await
            .context("failed to insert error result")?;

        Ok(())
    }
}

/// Operator-editable fields of a pending task. Only fields present (`Some`)
/// are changed; run-managed state is not represented here on purpose.
#[derive(Debug, Default, serde::Deserialize)]
pub struct TaskEdits {
    pub branch: Option<String>,
    pub default_branch: Option<String>,
    /// Operator override of the human lifecycle axis. Allowed on any task,
    /// regardless of its current state (unlike the branch fields).
    pub task_state: Option<String>,
}
