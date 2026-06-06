use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use sea_orm::*;
use tokio::sync::{Mutex, Semaphore};
use tracing::{info, error};
use uuid::Uuid;

use crate::config::Config;
use crate::entity::{task_results, tasks};
use crate::jobs::hub::LiveSessions;
use crate::jobs::output_log::TaskOutputLog;
use crate::jobs::registry::RunningTasks;
use crate::jobs::runner::run_job;
use crate::jobs::types::{ClaudeOutput, TriggerReason};
use crate::project::{ProjectStore, ProviderKind};
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
    output_log: TaskOutputLog,
    running: RunningTasks,
    hub: LiveSessions,
}

impl TaskStore {
    pub fn new(
        db: DatabaseConnection,
        config: Config,
        providers: ProviderRegistry,
        project_store: Arc<ProjectStore>,
        workspace: Arc<Workspace>,
        output_log: TaskOutputLog,
        running: RunningTasks,
        hub: LiveSessions,
    ) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_jobs)),
            seen_events: Arc::new(Mutex::new(HashSet::new())),
            db,
            config,
            providers,
            project_store,
            workspace,
            output_log,
            running,
            hub,
        }
    }

    pub fn output_log(&self) -> &TaskOutputLog {
        &self.output_log
    }

    pub fn hub(&self) -> &LiveSessions {
        &self.hub
    }

    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    /// Called at startup. Any task left in `running` or `pending` belongs to a
    /// previous process — the claude child died with us. Flip them to `killed`
    /// (and clear pid) so the UI shows them as pauseable, and the operator can
    /// Resume the ones that captured a session_id.
    pub async fn recover_orphans(&self) -> Result<u64> {
        let orphans = tasks::Entity::find()
            .filter(
                tasks::Column::Status
                    .eq("running")
                    .or(tasks::Column::Status.eq("pending")),
            )
            .all(&self.db)
            .await
            .context("query orphan tasks")?;

        let count = orphans.len() as u64;
        if count == 0 {
            return Ok(0);
        }

        for t in orphans {
            let id = t.id;
            let mut active: tasks::ActiveModel = t.into();
            active.status = Set("killed".to_string());
            active.finished_at = Set(Some(Utc::now().into()));
            active.pid = Set(None);
            if let Err(e) = active.update(&self.db).await {
                error!(%id, error = %e, "failed to mark orphan task killed");
            } else {
                info!(%id, "recovered orphan task → killed");
            }
        }
        Ok(count)
    }

    pub fn running(&self) -> &RunningTasks {
        &self.running
    }

    pub async fn kill_task(&self, task_id: Uuid) -> Result<()> {
        if !self.running.abort(task_id).await {
            anyhow::bail!("task is not running");
        }
        // The runner future was aborted before it could flush/close the live
        // session — do it here so the stdin pump stops and the WS detaches.
        self.hub.end(task_id).await;
        // Reflect the kill in the DB so the UI doesn't show "running" forever.
        let _ = self.finish_task(task_id, "killed").await;
        info!(%task_id, "aborted running task");
        Ok(())
    }

    pub async fn delete_task(&self, task_id: Uuid) -> Result<()> {
        // Force-kill if running. Abort drops the spawn's future, which drops
        // the claude Child (kill_on_drop=true) → SIGKILL.
        let was_running = self.running.abort(task_id).await;
        if was_running {
            self.hub.end(task_id).await;
            info!(%task_id, "aborted running task before delete");
        }

        match tasks::Entity::delete_by_id(task_id).exec(&self.db).await {
            Ok(res) if res.rows_affected == 0 => anyhow::bail!("task not found"),
            Ok(_) => Ok(()),
            Err(e) => {
                // Row delete failed after we killed. Leave the row in place but
                // flip status to "failed" so it doesn't show "running" forever.
                if was_running {
                    let _ = self.finish_task(task_id, "failed").await;
                }
                Err(anyhow::Error::new(e).context("failed to delete task"))
            }
        }
    }

    /// Resume a killed/failed/completed task in place. Same task row, same
    /// output buffer; spawns `claude -r <session_id>` so the streamed events
    /// append to whatever was already captured.
    pub async fn continue_task(self: &Arc<Self>, task_id: Uuid) -> Result<Uuid> {
        let task = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await
            .context("db error")?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;

        if task.session_id.is_none() {
            anyhow::bail!("task has no session_id; cannot resume");
        }
        if task.status == "running" || task.status == "pending" {
            anyhow::bail!("task is already {}; nothing to resume", task.status);
        }
        // A warm session is already live (idle between turns) — resuming would
        // spawn a second agent on the same branch. Message it instead.
        if self.hub.is_warm(task_id).await {
            anyhow::bail!("agent is still live for this task; send it a message instead of resuming");
        }

        // Reset the task row to pending so confirm_task can pick it up. Drop
        // any previous result row — claude will produce a fresh one.
        let mut active: tasks::ActiveModel = task.clone().into();
        active.status = Set("pending".to_string());
        active.finished_at = Set(None);
        active.started_at = Set(None);
        active.pid = Set(None);
        active.update(&self.db).await?;

        task_results::Entity::delete_many()
            .filter(task_results::Column::TaskId.eq(task_id))
            .exec(&self.db)
            .await
            .context("failed to clear previous task result")?;

        self.confirm_task(task_id).await?;
        Ok(task_id)
    }

    /// Queue an operator-supplied message to be used as the prompt on the
    /// next claude run for this task. If the task is currently running, we
    /// pause it (preserving session_id) and immediately resume so the message
    /// gets picked up. If it's already in a resumable state, just resume.
    pub async fn push_message(self: &Arc<Self>, task_id: Uuid, message: String) -> Result<()> {
        if message.trim().is_empty() {
            anyhow::bail!("message is empty");
        }

        // Warm path: a live agent is attached — hand the message straight to its
        // stdin. Its turn loop wakes, flips back to `running`, and processes it
        // with full in-session memory; no respawn, no delay.
        if self.hub.send_to_agent(task_id, &message).await {
            info!(%task_id, "delivered message to live agent");
            return Ok(());
        }

        // Cold path: no live agent. Persist the message and resume the session.
        let task = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await
            .context("db error")?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        if task.session_id.is_none() {
            anyhow::bail!("task has no session_id; cannot push a follow-up");
        }

        // Persist first so a crash before resume still delivers it next time.
        let mut active: tasks::ActiveModel = task.clone().into();
        active.pending_message = Set(Some(message));
        active.update(&self.db).await?;

        if task.status == "pending" {
            self.confirm_task(task_id).await?;
        } else {
            self.continue_task(task_id).await?;
        }
        Ok(())
    }

    async fn clear_pending_message(&self, task_id: Uuid) -> Result<()> {
        let mut active: tasks::ActiveModel = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?
            .into();
        active.pending_message = Set(None);
        active.update(&self.db).await?;
        Ok(())
    }

    /// Diff the task's working tree against `origin/<default_branch>`.
    /// Two-dot (not merge-base) so the result captures everything the operator
    /// would expect to see for an in-flight task: branch commits, staged
    /// edits, and unstaged edits. Untracked files are appended as a trailing
    /// `Untracked files:` listing — they don't show up in `git diff` but are
    /// part of "what's in this worktree right now".
    pub async fn branch_diff(&self, task_id: Uuid) -> Result<String> {
        use crate::workspace::layout::slugify;
        use tokio::process::Command;

        let task = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await
            .context("db error")?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;

        let Some(service_id) = task.git_service_id else {
            anyhow::bail!("task has no git_service_id");
        };
        let service = self
            .providers
            .service(service_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("git_service not loaded"))?;

        let project_slug = slugify(&task.project_path);
        let branch = task.branch.unwrap_or_else(|| task.default_branch.clone());
        let branch_slug = slugify(&branch);
        let work_dir = self
            .workspace
            .branch_dir(&service.slug, &project_slug, &branch_slug);

        if tokio::fs::metadata(&work_dir).await.is_err() {
            anyhow::bail!("branch checkout missing at {}", work_dir.display());
        }

        let base = format!("origin/{}", task.default_branch);
        let out = Command::new("git")
            .arg("-C")
            .arg(&work_dir)
            .arg("diff")
            .arg(&base)
            .output()
            .await
            .context("spawning git diff")?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            anyhow::bail!("git diff failed: {stderr}");
        }
        let mut diff = String::from_utf8_lossy(&out.stdout).into_owned();

        let untracked = Command::new("git")
            .arg("-C")
            .arg(&work_dir)
            .args(["ls-files", "--others", "--exclude-standard"])
            .output()
            .await
            .context("spawning git ls-files")?;
        if untracked.status.success() {
            let list = String::from_utf8_lossy(&untracked.stdout);
            let list = list.trim();
            if !list.is_empty() {
                if !diff.is_empty() && !diff.ends_with('\n') {
                    diff.push('\n');
                }
                diff.push_str("\nUntracked files:\n");
                for line in list.lines() {
                    diff.push_str("  ");
                    diff.push_str(line);
                    diff.push('\n');
                }
            }
        }

        Ok(diff)
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

    /// Edit a pending task's input fields. Run-managed fields (status,
    /// timestamps, session_id, pid, pending_message) are deliberately not
    /// editable — they're owned by the run loop. Only allowed before the task
    /// starts; once running, the worktree is already checked out. The resulting
    /// branch may never equal the default branch, so the "never run on default"
    /// rule holds.
    pub async fn update_task(&self, task_id: Uuid, edits: TaskEdits) -> Result<()> {
        let task = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        if task.status != "pending" {
            bail!(
                "can only edit a task while it is pending (status: {})",
                task.status
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
            None => task.branch.clone().unwrap_or_else(|| default_branch.clone()),
        };
        if branch == default_branch {
            bail!("refusing to set task branch to the default branch '{default_branch}'");
        }

        let mut active: tasks::ActiveModel = task.into();
        active.branch = Set(Some(branch));
        active.default_branch = Set(default_branch);
        active.update(&self.db).await?;
        Ok(())
    }

    pub async fn set_session_id_pub(&self, task_id: Uuid, session_id: &str) -> Result<()> {
        self.set_session_id(task_id, session_id).await
    }

    async fn set_session_id(&self, task_id: Uuid, session_id: &str) -> Result<()> {
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

    pub async fn create_task(
        &self,
        trigger: TriggerReason,
        git_service_id: Uuid,
        provider: ProviderKind,
        project_id: Option<Uuid>,
        project_path: String,
        git_url: String,
        default_branch: String,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let trigger_data = serde_json::to_value(&trigger)
            .context("failed to serialize trigger")?;

        // Every task runs on a non-default branch. MR triggers reuse the MR's
        // source branch; issue triggers derive `<iid>-<title-slug>`. A follow-up
        // comment carries no title, so it reuses the branch the original issue
        // task recorded (falling back to a bare `<iid>` if none exists yet).
        let branch = Some(match &trigger {
            TriggerReason::ReviewMR { source_branch, .. }
            | TriggerReason::FixReview { source_branch, .. }
            | TriggerReason::MRComment { source_branch, .. } => source_branch.clone(),
            TriggerReason::Issue { iid, title, .. } => issue_branch_name(*iid, title),
            TriggerReason::IssueComment { issue_iid, .. } => {
                let existing = match project_id {
                    Some(pid) => self
                        .project_store
                        .find_branch_for_issue(pid, *issue_iid as i64)
                        .await
                        .context("looking up issue branch")?
                        .map(|b| b.branch_name),
                    None => None,
                };
                existing.unwrap_or_else(|| issue_branch_name(*issue_iid, ""))
            }
        });
        let task = tasks::ActiveModel {
            id: Set(id),
            status: Set("pending".to_string()),
            trigger_type: Set(trigger.trigger_type().to_string()),
            trigger_data: Set(trigger_data),
            project_path: Set(project_path),
            git_url: Set(git_url),
            default_branch: Set(default_branch),
            created_at: Set(Utc::now().into()),
            started_at: Set(None),
            finished_at: Set(None),
            provider: Set(provider.as_str().to_string()),
            branch: Set(branch),
            project_id: Set(project_id),
            git_service_id: Set(Some(git_service_id)),
            session_id: Set(None),
            pid: Set(None),
            pending_message: Set(None),
        };

        tasks::Entity::insert(task)
            .exec(&self.db)
            .await
            .context("failed to insert task")?;

        info!(%id, "task created as pending");
        Ok(id)
    }

    /// Clone an existing task's trigger into a fresh pending row and immediately
    /// confirm it. Returns the new task's id.
    pub async fn retry_task(self: &Arc<Self>, task_id: Uuid) -> Result<Uuid> {
        let task = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await
            .context("db error")?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;

        let trigger: TriggerReason = serde_json::from_value(task.trigger_data.clone())
            .context("failed to deserialize trigger")?;
        let provider: ProviderKind = task.provider.parse()?;
        let service_id = task
            .git_service_id
            .ok_or_else(|| anyhow::anyhow!("task has no git_service_id; cannot retry"))?;

        let new_id = self
            .create_task(
                trigger,
                service_id,
                provider,
                task.project_id,
                task.project_path.clone(),
                task.git_url.clone(),
                task.default_branch.clone(),
            )
            .await?;

        self.confirm_task(new_id).await?;
        Ok(new_id)
    }

    pub async fn confirm_task(self: &Arc<Self>, task_id: Uuid) -> Result<()> {
        let task = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await
            .context("db error")?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;

        if task.status != "pending" {
            bail!("task is not pending (status: {})", task.status);
        }

        // One agent per branch: refuse to start while another task on the same
        // project *and branch* is already running — that's the real conflict,
        // since both would share one worktree. Different branches each have
        // their own clone and run concurrently. The branch lock serializes
        // setup anyway, but blocking up-front gives the operator a clear error
        // instead of a task that silently waits.
        if let (Some(pid), Some(branch)) = (task.project_id, task.branch.clone()) {
            let other = tasks::Entity::find()
                .filter(tasks::Column::ProjectId.eq(pid))
                .filter(tasks::Column::Branch.eq(branch.clone()))
                .filter(tasks::Column::Status.eq("running"))
                .filter(tasks::Column::Id.ne(task_id))
                .one(&self.db)
                .await
                .context("checking concurrent branch task")?;
            if let Some(other) = other {
                bail!(
                    "another task ({}) is already running on branch '{branch}'; \
                     wait for it to finish or kill it first",
                    other.id
                );
            }
        }

        let store = Arc::clone(self);
        let semaphore = self.semaphore.clone();

        // The job spawn no longer holds a permit for its whole life — run_job
        // acquires one per *turn*, so an idle (warm) agent occupies no slot.
        // Setup failures here mark the task failed; run_job owns the rest of the
        // lifecycle (status transitions, per-turn results, final finish).
        let join = tokio::spawn(async move {
            if let Err(e) = store.set_status(task_id, "running").await {
                error!(%task_id, error = %e, "failed to set running status");
                return;
            }

            let trigger: TriggerReason = match serde_json::from_value(task.trigger_data.clone()) {
                Ok(t) => t,
                Err(e) => {
                    error!(%task_id, error = %e, "failed to deserialize trigger");
                    let _ = store.finish_task(task_id, "failed").await;
                    return;
                }
            };

            let Some(service_id) = task.git_service_id else {
                error!(%task_id, "task has no git_service_id");
                let _ = store.finish_task(task_id, "failed").await;
                return;
            };

            let service = match store.providers.service(service_id).await {
                Some(s) => s,
                None => {
                    error!(%task_id, %service_id, "git_service not loaded");
                    let _ = store.finish_task(task_id, "failed").await;
                    return;
                }
            };

            let provider = match store.providers.require(service_id).await {
                Ok(p) => p,
                Err(e) => {
                    error!(%task_id, error = %e, "provider not configured");
                    let _ = store.finish_task(task_id, "failed").await;
                    return;
                }
            };

            info!(%task_id, "job starting");

            let resume_session_id = task.session_id.clone();
            // Consume pending_message: clear the column before the run so a
            // crash doesn't replay the same message, and pass it as the prompt.
            let prompt_override = task.pending_message.clone();
            if prompt_override.is_some() {
                if let Err(e) = store.clear_pending_message(task_id).await {
                    error!(%task_id, error = %e, "failed to clear pending_message");
                }
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
                store.output_log.clone(),
                store.hub.clone(),
                store.clone(),
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
                let _ = store.finish_task(task_id, "failed").await;
            }
        });

        // Register AFTER spawn — claude takes hundreds of ms minimum to start
        // up, so the operator will always have a window to kill. If the task
        // somehow finishes before this line runs, the abort handle is harmless.
        self.running.register(task_id, join.abort_handle()).await;

        Ok(())
    }

    /// Set the task's status without touching `finished_at` — used for the
    /// per-turn running↔completed transitions of a warm session. `started_at`
    /// is stamped once, on the first move to `running`.
    pub(crate) async fn set_status(&self, task_id: Uuid, status: &str) -> Result<()> {
        let model = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        let stamp_start = status == "running" && model.started_at.is_none();
        let mut task: tasks::ActiveModel = model.into();
        task.status = Set(status.to_string());
        if stamp_start {
            task.started_at = Set(Some(Utc::now().into()));
        }
        task.update(&self.db).await?;
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

    pub(crate) async fn finish_task(&self, task_id: Uuid, status: &str) -> Result<()> {
        let mut task: tasks::ActiveModel = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?
            .into();

        task.status = Set(status.to_string());
        task.finished_at = Set(Some(Utc::now().into()));
        task.update(&self.db).await?;
        Ok(())
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

    pub async fn list_tasks(&self, status: Option<&str>) -> Result<Vec<tasks::Model>> {
        let mut query = tasks::Entity::find()
            .order_by_desc(tasks::Column::CreatedAt);

        if let Some(status) = status {
            query = query.filter(tasks::Column::Status.eq(status));
        }

        query
            .all(&self.db)
            .await
            .context("failed to list tasks")
    }

    /// Persisted agent event history from `task_events`, ordered by `seq`.
    pub async fn task_events(&self, task_id: Uuid) -> Result<Vec<serde_json::Value>> {
        use crate::entity::task_events;
        let rows = task_events::Entity::find()
            .filter(task_events::Column::TaskId.eq(task_id))
            .order_by_asc(task_events::Column::Seq)
            .all(&self.db)
            .await
            .context("loading task events")?;
        Ok(rows.into_iter().map(|r| r.payload).collect())
    }

    pub async fn get_task(
        &self,
        task_id: Uuid,
    ) -> Result<Option<(tasks::Model, Option<task_results::Model>)>> {
        let task = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await
            .context("db error")?;

        let Some(task) = task else {
            return Ok(None);
        };

        let result = task_results::Entity::find()
            .filter(task_results::Column::TaskId.eq(task_id))
            .one(&self.db)
            .await
            .context("db error")?;

        Ok(Some((task, result)))
    }
}

/// Operator-editable fields of a pending task. Only fields present (`Some`)
/// are changed; run-managed state is not represented here on purpose.
#[derive(Debug, Default, serde::Deserialize)]
pub struct TaskEdits {
    pub branch: Option<String>,
    pub default_branch: Option<String>,
}

/// `<iid>-<slug(title)>`, e.g. `42-fix-login-button`. Falls back to a bare
/// `<iid>` when the title slug is empty. The slug is capped so branch names
/// stay sane for long issue titles.
fn issue_branch_name(iid: u64, title: &str) -> String {
    use crate::workspace::layout::slugify;
    let mut slug = slugify(title);
    slug.truncate(50);
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        iid.to_string()
    } else {
        format!("{iid}-{slug}")
    }
}
