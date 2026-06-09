//! Operator control of a task's lifecycle: pause (kill), delete, resume
//! (continue), and follow-up messaging. Split out of `store.rs` (over the
//! 400-line cap) along the operator-action seam.

use std::sync::Arc;

use anyhow::{Context, Result};
use sea_orm::*;
use tracing::info;
use uuid::Uuid;

use crate::entity::{task_sessions, tasks};
use crate::jobs::lifecycle::{
    AGENT_COLD, AGENT_FAILED, AGENT_PENDING, TASK_FAILED, TASK_WORKING_ON,
};
use crate::jobs::store::TaskStore;

impl TaskStore {
    /// Operator Pause. SIGKILL the live agent, detach the session, and record the
    /// pause as a result note. The work isn't done — durable agent_state goes
    /// back to `cold` (no live agent) but task_state stays `working_on` so the
    /// operator sees it as resumable.
    pub async fn kill_task(&self, task_id: Uuid) -> Result<()> {
        if !self.running().abort(task_id).await {
            anyhow::bail!("task is not running");
        }
        // The runner future was aborted before it could flush/close the live
        // session — do it here so the stdin pump stops and the WS detaches.
        self.hub().end(task_id).await;
        let _ = self
            .finish_task(
                task_id,
                AGENT_COLD,
                TASK_WORKING_ON,
                Some("paused by operator"),
            )
            .await;
        info!(%task_id, "aborted running task (paused)");
        Ok(())
    }

    pub async fn delete_task(&self, task_id: Uuid) -> Result<()> {
        // Force-kill if running. Abort drops the spawn's future, which drops
        // the claude Child (kill_on_drop=true) → SIGKILL.
        let was_running = self.running().abort(task_id).await;
        if was_running {
            self.hub().end(task_id).await;
            info!(%task_id, "aborted running task before delete");
        }

        match tasks::Entity::delete_by_id(task_id).exec(self.db()).await {
            Ok(res) if res.rows_affected == 0 => anyhow::bail!("task not found"),
            Ok(_) => Ok(()),
            Err(e) => {
                // Row delete failed after we killed. Leave the row in place but
                // flip it to failed so it doesn't show as live forever.
                if was_running {
                    let _ = self
                        .finish_task(task_id, AGENT_FAILED, TASK_FAILED, None)
                        .await;
                }
                Err(anyhow::Error::new(e).context("failed to delete task"))
            }
        }
    }

    /// Resume a paused/failed/completed task in place. Same task row, same
    /// output buffer; spawns `claude -r <session_id>` so the streamed events
    /// append to whatever was already captured.
    pub async fn continue_task(self: &Arc<Self>, task_id: Uuid) -> Result<Uuid> {
        let task = tasks::Entity::find_by_id(task_id)
            .one(self.db())
            .await
            .context("db error")?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;

        if task.session_id.is_none() {
            anyhow::bail!("task has no session_id; cannot resume");
        }
        // Never resumable while a live agent is attached (warm/idle or actively
        // running) — that would spawn a second agent on the same branch; message
        // it instead. A never-started task (task_state pending) has nothing to
        // resume yet.
        if self.hub().is_warm(task_id).await || self.hub().is_running(task_id) {
            anyhow::bail!(
                "agent is still live for this task; send it a message instead of resuming"
            );
        }
        if task.task_state == "pending" {
            anyhow::bail!("task is still pending; confirm it instead of resuming");
        }

        // Reset run-managed fields so confirm_task can pick it up. The durable
        // agent_state goes to `pending` (queued to spawn) and task_state to
        // `working_on` (it's about to work again). Drop any previous result row —
        // claude will produce a fresh one.
        let mut active: tasks::ActiveModel = task.clone().into();
        active.agent_state = Set(AGENT_PENDING.to_string());
        active.task_state = Set(TASK_WORKING_ON.to_string());
        active.finished_at = Set(None);
        active.started_at = Set(None);
        active.pid = Set(None);
        active.update(self.db()).await?;

        task_sessions::Entity::delete_many()
            .filter(task_sessions::Column::TaskId.eq(task_id))
            .exec(self.db())
            .await
            .context("failed to clear previous task result")?;

        self.confirm_task(task_id).await?;
        Ok(task_id)
    }

    /// Queue an operator-supplied message as the prompt for this task's next
    /// turn. A warm agent takes it straight over stdin; otherwise it's persisted
    /// and the session is (re)started — confirmed if never run, resumed if it has
    /// a captured session.
    pub async fn push_message(self: &Arc<Self>, task_id: Uuid, message: String) -> Result<()> {
        if message.trim().is_empty() {
            anyhow::bail!("message is empty");
        }

        // Warm path: a live agent is attached — hand the message straight to its
        // stdin. Its turn loop wakes, flips back to `running`, and processes it
        // with full in-session memory; no respawn, no delay.
        if self.hub().send_to_agent(task_id, &message).await {
            info!(%task_id, "delivered message to live agent");
            return Ok(());
        }

        // Cold path: no live agent. Persist the message and resume the session.
        let task = tasks::Entity::find_by_id(task_id)
            .one(self.db())
            .await
            .context("db error")?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        if task.session_id.is_none() {
            anyhow::bail!("task has no session_id; cannot push a follow-up");
        }

        // Persist first so a crash before resume still delivers it next time.
        let mut active: tasks::ActiveModel = task.clone().into();
        active.pending_message = Set(Some(message));
        active.update(self.db()).await?;

        if task.task_state == "pending" {
            self.confirm_task(task_id).await?;
        } else {
            self.continue_task(task_id).await?;
        }
        Ok(())
    }

    pub(crate) async fn clear_pending_message(&self, task_id: Uuid) -> Result<()> {
        let mut active: tasks::ActiveModel = tasks::Entity::find_by_id(task_id)
            .one(self.db())
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?
            .into();
        active.pending_message = Set(None);
        active.update(self.db()).await?;
        Ok(())
    }
}
