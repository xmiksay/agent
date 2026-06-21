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
use crate::entity::tasks;
use crate::jobs::hub::LiveSessions;
use crate::jobs::lifecycle::{AGENT_FAILED, AGENT_PENDING, TASK_FAILED};
use crate::jobs::registry::RunningTasks;
use crate::jobs::runner::{RunJobContext, run_job};
use crate::jobs::types::TriggerReason;
use crate::models::ModelStore;
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
    model_store: ModelStore,
    workspace: Arc<Workspace>,
    running: RunningTasks,
    hub: LiveSessions,
    auth_store: Arc<AuthStore>,
    auth_waiter: AuthWaiter,
    /// Serializes queue admission: held across the free-slot count read and the
    /// `confirm_task` that fills it, so two concurrent completions can't both
    /// grab the last slot. See `jobs::queue::try_admit_next`.
    admit_lock: Arc<Mutex<()>>,
    /// Wakes the admission loop (`run_admission_loop`) when a slot may have
    /// freed. A `Notify`, not a direct `try_admit_next()` call, so the run
    /// future's spawned closure can signal admission without closing the
    /// `confirm_task` → `try_admit_next` async-recursion cycle.
    admit_notify: Arc<tokio::sync::Notify>,
}

/// Dependencies wired into a `TaskStore` at construction. Grouped so the
/// constructor takes one value instead of a nine-deep positional argument list.
pub struct TaskStoreDeps {
    pub db: DatabaseConnection,
    pub config: Config,
    pub providers: ProviderRegistry,
    pub project_store: Arc<ProjectStore>,
    pub workspace: Arc<Workspace>,
    pub running: RunningTasks,
    pub hub: LiveSessions,
    pub auth_store: Arc<AuthStore>,
    pub auth_waiter: AuthWaiter,
}

impl TaskStore {
    pub fn new(deps: TaskStoreDeps) -> Self {
        let TaskStoreDeps {
            db,
            config,
            providers,
            project_store,
            workspace,
            running,
            hub,
            auth_store,
            auth_waiter,
        } = deps;
        Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_jobs)),
            seen_events: Arc::new(Mutex::new(HashSet::new())),
            model_store: ModelStore::new(db.clone()),
            db,
            config,
            providers,
            project_store,
            workspace,
            running,
            hub,
            auth_store,
            auth_waiter,
            admit_lock: Arc::new(Mutex::new(())),
            admit_notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Tokio semaphore size — the queue's free-slot ceiling. See
    /// `jobs::queue::try_admit_next`.
    pub(crate) fn max_concurrent_jobs(&self) -> usize {
        self.config.max_concurrent_jobs
    }

    pub(crate) fn admit_lock(&self) -> &Arc<Mutex<()>> {
        &self.admit_lock
    }

    /// Signal that a task-admission slot may have freed, waking the admission
    /// loop. Cheap and non-async — safe to call from the run future's spawned
    /// closure (where awaiting `try_admit_next` directly would not compile).
    pub fn request_admit(&self) {
        self.admit_notify.notify_one();
    }

    /// Long-lived loop that pulls queued tasks into free slots whenever signalled
    /// (via `request_admit`). Spawned once at startup. Owning the recursion here —
    /// rather than in the run future's closure — keeps `confirm_task`'s spawned
    /// future free of the `try_admit_next` cycle.
    pub async fn run_admission_loop(self: Arc<Self>) {
        loop {
            self.admit_notify.notified().await;
            self.try_admit_next().await;
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

    pub fn model_store(&self) -> &ModelStore {
        &self.model_store
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
        if let Some(branch) = task.branch.clone()
            && let Some(other) = self
                .branch_is_live(task.project_id, &branch, task_id)
                .await?
        {
            bail!(
                "another task ({other}) is already live on branch '{branch}'; \
                 wait for it to finish or kill it first"
            );
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

            // Resolve the run's model (joined to its provider): the task's pick,
            // else the global default (else the CLI's own default).
            let model = store.resolve_model(task.model_id).await;

            let resume_session_id = task.session_id.clone();
            // Consume pending_message: clear the column before the run so a
            // crash doesn't replay the same message, and pass it as the prompt.
            let prompt_override = task.pending_message.clone();
            if prompt_override.is_some()
                && let Err(e) = store.clear_pending_message(task_id).await
            {
                error!(%task_id, error = %e, "failed to clear pending_message");
            }

            let result = run_job(RunJobContext {
                task_id,
                trigger,
                service: service.clone(),
                project_id: Some(task.project_id),
                git_url: project.remote_url.clone(),
                project_path: project.full_name.clone(),
                default_branch: project.default_branch.clone(),
                branch_override: task.branch.clone(),
                config: store.config.clone(),
                provider,
                workspace: store.workspace.clone(),
                project_store: store.project_store.clone(),
                hub: store.hub.clone(),
                store: store.clone(),
                auth_store: store.auth_store.clone(),
                auth_waiter: store.auth_waiter.clone(),
                semaphore,
                resume_session_id,
                prompt_override,
                model,
            })
            .await;

            store.running.unregister(task_id).await;
            // The run future ending is the authoritative moment a task-admission
            // slot frees. Signal the admission loop rather than calling
            // try_admit_next() here: that would close a confirm_task →
            // try_admit_next → confirm_task async cycle the spawn's Send bound
            // can't satisfy. The loop owns the actual pull.
            store.request_admit();

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
}
