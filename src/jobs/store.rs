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
use crate::entity::{task_sessions, tasks};
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

        let edits_branch = edits.branch.is_some();
        if edits_branch && task.task_state != "pending" {
            bail!(
                "can only edit the branch while the task is pending (task_state: {})",
                task.task_state
            );
        }

        let new_task_state = edits.task_state.clone();
        let project_id = task.project_id;
        let mut active: tasks::ActiveModel = task.into();
        if let Some(b) = edits.branch.as_deref().map(str::trim) {
            if b.is_empty() {
                bail!("branch must not be empty");
            }
            // The default branch is a project property now — load it for the
            // "never run on the default branch" guard.
            let default_branch = self
                .project_store
                .get_project_by_id(project_id)
                .await?
                .map(|p| p.default_branch)
                .unwrap_or_default();
            if b == default_branch {
                bail!("refusing to set task branch to the default branch '{default_branch}'");
            }
            active.branch = Set(Some(b.to_string()));
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
        if let Some(branch) = task.branch.clone() {
            let pid = task.project_id;
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

            // Where/how the task runs is resolved through its project, not stored
            // on the task: project → remote/default_branch/provider, and the owning
            // service for tokens + notes.
            let project = match store.project_store.get_project_by_id(task.project_id).await {
                Ok(Some(p)) => p,
                _ => {
                    error!(%task_id, project_id = %task.project_id, "task project not found");
                    let _ = fail().await;
                    return;
                }
            };

            let Some(service_id) = project.service_id else {
                error!(%task_id, "task project has no service");
                let _ = fail().await;
                return;
            };

            let service = match store.providers.service(service_id).await {
                Some(s) => s,
                None => {
                    error!(%task_id, %service_id, "service not loaded");
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
                Some(task.project_id),
                project.remote_url.clone(),
                project.full_name.clone(),
                project.default_branch.clone(),
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

    /// Record the current run's metrics. Sessions are 1:N per task: turns within
    /// one agent run (same `session_id`) accumulate into a single row, while a new
    /// run (fresh `session_id`) starts a new row, so the history is preserved.
    pub(crate) async fn replace_result(&self, task_id: Uuid, output: &ClaudeOutput) -> Result<()> {
        if !output.session_id.is_empty()
            && let Some(existing) = task_sessions::Entity::find()
                .filter(task_sessions::Column::TaskId.eq(task_id))
                .filter(task_sessions::Column::SessionId.eq(output.session_id.clone()))
                .one(&self.db)
                .await
                .context("looking up session row")?
        {
            let mut active: task_sessions::ActiveModel = existing.into();
            active.cost_usd = Set(output.total_cost_usd);
            active.input_tokens = Set(output.input_tokens as i64);
            active.output_tokens = Set(output.output_tokens as i64);
            active.num_turns = Set(output.num_turns as i32);
            active.is_error = Set(output.is_error);
            active.result_text = Set(output.result.clone());
            active
                .update(&self.db)
                .await
                .context("updating session row")?;
            return Ok(());
        }
        self.save_result(task_id, output).await
    }

    async fn save_result(&self, task_id: Uuid, output: &ClaudeOutput) -> Result<()> {
        let result = task_sessions::ActiveModel {
            id: Set(Uuid::new_v4()),
            task_id: Set(task_id),
            cost_usd: Set(output.total_cost_usd),
            input_tokens: Set(output.input_tokens as i64),
            output_tokens: Set(output.output_tokens as i64),
            num_turns: Set(output.num_turns as i32),
            is_error: Set(output.is_error),
            result_text: Set(output.result.clone()),
            session_id: Set(output.session_id.clone()),
            created_at: Set(chrono::Utc::now().into()),
        };

        task_sessions::Entity::insert(result)
            .exec(&self.db)
            .await
            .context("failed to insert task session")?;

        Ok(())
    }

    async fn save_error_result(&self, task_id: Uuid, error: &str) -> Result<()> {
        let result = task_sessions::ActiveModel {
            id: Set(Uuid::new_v4()),
            task_id: Set(task_id),
            cost_usd: Set(0.0),
            input_tokens: Set(0),
            output_tokens: Set(0),
            num_turns: Set(0),
            is_error: Set(true),
            result_text: Set(error.to_string()),
            session_id: Set(String::new()),
            created_at: Set(chrono::Utc::now().into()),
        };

        task_sessions::Entity::insert(result)
            .exec(&self.db)
            .await
            .context("failed to insert error session")?;

        Ok(())
    }
}

/// Operator-editable fields of a pending task. Only fields present (`Some`)
/// are changed; run-managed state is not represented here on purpose.
#[derive(Debug, Default, serde::Deserialize)]
pub struct TaskEdits {
    pub branch: Option<String>,
    /// Operator override of the human lifecycle axis. Allowed on any task,
    /// regardless of its current state (unlike the branch fields).
    pub task_state: Option<String>,
}
