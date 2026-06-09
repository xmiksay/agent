//! Read-side `TaskStore` methods: task lookups, listings, the resumable-branch
//! lookup, persisted event history, and the worktree diff. Split out of
//! `store.rs` (over the 400-line cap) along the read/query seam — these touch no
//! run-loop state.

use anyhow::{Context, Result};
use sea_orm::*;
use uuid::Uuid;

use crate::entity::{task_results, tasks};
use crate::jobs::store::TaskStore;

impl TaskStore {
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
            .one(self.db())
            .await
            .context("db error")?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;

        let Some(service_id) = task.service_id else {
            anyhow::bail!("task has no service_id");
        };
        let service = self
            .providers()
            .service(service_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("service not loaded"))?;

        let project_slug = slugify(&task.project_path);
        let branch = task.branch.unwrap_or_else(|| task.default_branch.clone());
        let branch_slug = slugify(&branch);
        let work_dir = self
            .workspace()
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

    /// List tasks, optionally filtered by the persisted `task_state`. The
    /// derived `agent_state` is overlaid at the API layer and (when filtered on)
    /// applied in-memory there, since it isn't a column.
    pub async fn list_tasks(&self, task_state: Option<&str>) -> Result<Vec<tasks::Model>> {
        let mut query = tasks::Entity::find().order_by_desc(tasks::Column::CreatedAt);

        if let Some(task_state) = task_state {
            query = query.filter(tasks::Column::TaskState.eq(task_state));
        }

        query.all(self.db()).await.context("failed to list tasks")
    }

    /// Most recent task on this project+branch that captured a `session_id`, so
    /// it can take a follow-up (delivered live to a warm agent, or via resume).
    /// Lets a comment continue the same agent/session instead of spawning a fresh
    /// one on the shared branch.
    pub async fn find_resumable_task_for_branch(
        &self,
        project_id: Uuid,
        branch: &str,
    ) -> Result<Option<Uuid>> {
        let row = tasks::Entity::find()
            .filter(tasks::Column::ProjectId.eq(project_id))
            .filter(tasks::Column::Branch.eq(branch))
            .filter(tasks::Column::SessionId.is_not_null())
            .order_by_desc(tasks::Column::CreatedAt)
            .one(self.db())
            .await
            .context("looking up resumable task for branch")?;
        Ok(row.map(|t| t.id))
    }

    /// Persisted hub-frame history from `task_events`, ordered by `seq`. Carries
    /// every frame kind (event / auth_request / status) with its `seq` + `kind`.
    pub async fn task_events(
        &self,
        task_id: Uuid,
    ) -> Result<Vec<crate::entity::task_events::Model>> {
        use crate::entity::task_events;
        task_events::Entity::find()
            .filter(task_events::Column::TaskId.eq(task_id))
            .order_by_asc(task_events::Column::Seq)
            .all(self.db())
            .await
            .context("loading task events")
    }

    pub async fn get_task(
        &self,
        task_id: Uuid,
    ) -> Result<Option<(tasks::Model, Option<task_results::Model>)>> {
        let task = tasks::Entity::find_by_id(task_id)
            .one(self.db())
            .await
            .context("db error")?;

        let Some(task) = task else {
            return Ok(None);
        };

        let result = task_results::Entity::find()
            .filter(task_results::Column::TaskId.eq(task_id))
            .one(self.db())
            .await
            .context("db error")?;

        Ok(Some((task, result)))
    }
}
