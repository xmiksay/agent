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
use crate::project::ProviderKind;

impl TaskStore {
    #[allow(clippy::too_many_arguments)]
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
            TriggerReason::IssueComment { issue_iid, .. } => {
                let existing = match project_id {
                    Some(pid) => self
                        .project_store()
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
            agent_state: Set(AGENT_COLD.to_string()),
            task_state: Set(TASK_PENDING.to_string()),
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
            .exec(self.db())
            .await
            .context("failed to insert task")?;

        info!(%id, "task created as pending");
        Ok(id)
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
