//! Operator edits to a task. Lifecycle (`task_state`) is editable any time; the
//! run *input* fields (branch, title/description, queue) are pending-only.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use sea_orm::*;
use uuid::Uuid;

use crate::entity::tasks;
use crate::jobs::lifecycle::TASK_STATES;
use crate::jobs::store::TaskStore;
use crate::jobs::types::TriggerReason;

impl TaskStore {
    /// Edit a task. The operator may set `task_state` (the human lifecycle) on
    /// ANY task regardless of its current state. The run *input* fields — the
    /// `branch` and the trigger's `title`/`description` (which drive the prompt)
    /// — are only editable while the task hasn't started yet (`task_state ==
    /// "pending"`), i.e. it isn't related to a run: once running, the worktree is
    /// checked out and the prompt already built. The durable `agent_state` and
    /// other run-managed fields (timestamps, session_id, pid, pending_message)
    /// are never editable. The resulting branch may never equal the default
    /// branch, so the "never run on default" rule holds.
    pub async fn update_task(self: &Arc<Self>, task_id: Uuid, edits: TaskEdits) -> Result<()> {
        let task = tasks::Entity::find_by_id(task_id)
            .one(self.db())
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

        // Branch/title/description drive the prompt and are only editable before
        // the run starts — once running the worktree is checked out and the
        // prompt is built. The model override is exempt: it's read fresh at each
        // spawn, so changing it on a non-pending task takes effect on the next
        // run/resume (#51).
        let edits_input = edits.branch.is_some()
            || edits.title.is_some()
            || edits.description.is_some()
            || edits.queue_id.is_some()
            || edits.priority.is_some();
        if edits_input && task.task_state != "pending" {
            bail!(
                "can only edit the branch, title, description or queue while the task is pending \
                 (task_state: {})",
                task.task_state
            );
        }

        let new_task_state = edits.task_state.clone();
        let project_id = task.project_id;
        // Patch the trigger's title/description in place: round-trip the JSON,
        // overwrite the existing keys, then validate the result still
        // deserializes as a TriggerReason so the prompt builder never chokes.
        let mut trigger_data = task.trigger_data.clone();
        if edits.title.is_some() || edits.description.is_some() {
            let obj = trigger_data
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("trigger_data is not a JSON object"))?;
            if let Some(title) = edits.title.as_deref().map(str::trim) {
                if title.is_empty() {
                    bail!("title must not be empty");
                }
                if !obj.contains_key("title") {
                    bail!("this trigger type has no title to edit");
                }
                obj.insert("title".into(), serde_json::Value::String(title.to_string()));
            }
            if let Some(description) = edits.description.as_deref() {
                if !obj.contains_key("description") {
                    bail!("this trigger type has no description to edit");
                }
                obj.insert(
                    "description".into(),
                    serde_json::Value::String(description.to_string()),
                );
            }
            serde_json::from_value::<TriggerReason>(trigger_data.clone())
                .context("edited trigger_data is no longer a valid trigger")?;
        }
        let mut active: tasks::ActiveModel = task.into();
        if edits.title.is_some() || edits.description.is_some() {
            active.trigger_data = Set(trigger_data);
        }
        if let Some(b) = edits.branch.as_deref().map(str::trim) {
            if b.is_empty() {
                bail!("branch must not be empty");
            }
            // The default branch is a project property now — load it for the
            // "never run on the default branch" guard.
            let default_branch = self
                .project_store()
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
        if let Some(model_id) = edits.model_id {
            active.model_id = Set(model_id);
        }
        if let Some(queue_id) = edits.queue_id {
            active.queue_id = Set(queue_id);
        }
        if let Some(priority) = edits.priority {
            active.priority = Set(priority);
        }
        active.update(self.db()).await?;
        // Enqueuing (or re-prioritizing) a pending task may make it the next
        // admittable one — pull it in if a slot is free.
        if edits.queue_id.is_some() || edits.priority.is_some() {
            self.try_admit_next().await;
        }
        Ok(())
    }
}

/// Operator-editable fields of a task. Only fields present (`Some`) are changed;
/// run-managed state is not represented here on purpose.
#[derive(Debug, Default, serde::Deserialize)]
pub struct TaskEdits {
    /// Working branch — pending-only (the worktree is checked out once a run
    /// starts).
    pub branch: Option<String>,
    /// Operator override of the human lifecycle axis. Allowed on any task,
    /// regardless of its current state (unlike the input fields below).
    pub task_state: Option<String>,
    /// The trigger's title — pending-only (it drives the prompt, built once at
    /// run start). Only triggers that carry a title accept this.
    pub title: Option<String>,
    /// The trigger's description — pending-only, same reason. Only triggers that
    /// carry a description (issue triggers) accept this.
    pub description: Option<String>,
    /// Operator override of the task's model. Editable in any state (unlike
    /// `branch`/title/description) — it's read fresh at each spawn, so a change
    /// takes effect on the next run/resume (#51). Outer `None` = leave unchanged;
    /// `Some(None)` = clear (revert to the service/global default at run time);
    /// `Some(Some(id))` = set it.
    #[serde(default, deserialize_with = "crate::service::store::double_option")]
    pub model_id: Option<Option<Uuid>>,
    /// Enqueue/dequeue the task — pending-only (only a not-yet-started task is a
    /// scheduling candidate). Outer `None` = leave unchanged; `Some(None)` =
    /// remove from its queue; `Some(Some(id))` = put it in that queue.
    #[serde(default, deserialize_with = "crate::service::store::double_option")]
    pub queue_id: Option<Option<Uuid>>,
    /// In-queue sort priority (higher = sooner) — pending-only. Only meaningful
    /// alongside a `queue_id`.
    pub priority: Option<i16>,
}
