//! Task creation: deriving the per-trigger branch, inserting a fresh `pending`
//! row, and the retry clone. Split out of `store.rs` (over the 400-line cap).

use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use sea_orm::*;
use tracing::info;
use uuid::Uuid;

use crate::entity::tasks;
use crate::jobs::lifecycle::{AGENT_COLD, TASK_PENDING};
use crate::jobs::store::TaskStore;
use crate::jobs::types::TriggerReason;

impl TaskStore {
    /// Create a fresh `pending` task for `project_id`. Where/how it runs (remote,
    /// default branch, provider, owning service) is resolved from the project at
    /// run time, not stored on the task.
    pub async fn create_task(&self, trigger: TriggerReason, project_id: Uuid) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let trigger_data = serde_json::to_value(&trigger).context("failed to serialize trigger")?;

        // Every task runs on a non-default branch. MR triggers reuse the MR's
        // source branch; issue triggers derive `<iid>-<title-slug>`. A follow-up
        // comment carries no title, so it reuses the branch the original issue
        // task recorded (falling back to a bare `<iid>` if none exists yet).
        let branch = Some(match &trigger {
            TriggerReason::ReviewMR { source_branch, .. }
            | TriggerReason::FixReview { source_branch, .. }
            | TriggerReason::MRComment { source_branch, .. } => source_branch.clone(),
            TriggerReason::Issue { iid, title, .. } => issue_branch_name(*iid, title),
            TriggerReason::IssueComment { issue_iid, .. } => self
                .project_store()
                .find_branch_for_issue(project_id, *issue_iid as i64)
                .await
                .context("looking up issue branch")?
                .map(|b| b.branch_name)
                .unwrap_or_else(|| issue_branch_name(*issue_iid, "")),
        });
        let task = tasks::ActiveModel {
            id: Set(id),
            agent_state: Set(AGENT_COLD.to_string()),
            task_state: Set(TASK_PENDING.to_string()),
            trigger_type: Set(trigger.trigger_type().to_string()),
            trigger_data: Set(trigger_data),
            created_at: Set(Utc::now().into()),
            started_at: Set(None),
            finished_at: Set(None),
            branch: Set(branch),
            project_id: Set(project_id),
            session_id: Set(None),
            pid: Set(None),
            pending_message: Set(None),
        };

        tasks::Entity::insert(task)
            .exec(self.db())
            .await
            .context("failed to insert task")?;

        info!(%id, "task created as pending");
        Ok(id)
    }

    /// Refresh an existing issue task's stored title + description in place — the
    /// dedup path (issue #35) for a re-fired/edited issue webhook. Only the
    /// trigger payload that the next run (or resume) reads is rewritten; the
    /// task's state, branch, and session are left untouched.
    pub async fn update_issue_description(
        &self,
        task_id: Uuid,
        title: &str,
        description: &str,
    ) -> Result<()> {
        let task = tasks::Entity::find_by_id(task_id)
            .one(self.db())
            .await
            .context("db error")?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;

        let mut trigger: TriggerReason = serde_json::from_value(task.trigger_data.clone())
            .context("failed to deserialize trigger")?;
        let TriggerReason::Issue {
            title: t,
            description: d,
            ..
        } = &mut trigger
        else {
            anyhow::bail!("task {task_id} is not an issue task");
        };
        *t = title.to_string();
        *d = description.to_string();

        let trigger_data = serde_json::to_value(&trigger).context("failed to serialize trigger")?;
        let mut active: tasks::ActiveModel = task.into();
        active.trigger_data = Set(trigger_data);
        active
            .update(self.db())
            .await
            .context("updating issue trigger")?;
        Ok(())
    }

    /// Clone an existing task's trigger into a fresh pending row and immediately
    /// confirm it. Returns the new task's id.
    pub async fn retry_task(self: &Arc<Self>, task_id: Uuid) -> Result<Uuid> {
        let task = tasks::Entity::find_by_id(task_id)
            .one(self.db())
            .await
            .context("db error")?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;

        let trigger: TriggerReason = serde_json::from_value(task.trigger_data.clone())
            .context("failed to deserialize trigger")?;

        let new_id = self.create_task(trigger, task.project_id).await?;

        self.confirm_task(new_id).await?;
        Ok(new_id)
    }
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
