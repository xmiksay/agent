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
use crate::gitlab::client::GitLabClient;
use crate::jobs::runner::run_job;
use crate::jobs::types::{ClaudeOutput, TriggerReason};

pub struct TaskStore {
    db: DatabaseConnection,
    semaphore: Arc<Semaphore>,
    seen_events: Arc<Mutex<HashSet<String>>>,
    config: Config,
    gitlab: GitLabClient,
}

impl TaskStore {
    pub fn new(db: DatabaseConnection, config: Config, gitlab: GitLabClient) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_jobs)),
            seen_events: Arc::new(Mutex::new(HashSet::new())),
            db,
            config,
            gitlab,
        }
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
        project_path: String,
        git_url: String,
        default_branch: String,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let trigger_data = serde_json::to_value(&trigger)
            .context("failed to serialize trigger")?;

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
        };

        tasks::Entity::insert(task)
            .exec(&self.db)
            .await
            .context("failed to insert task")?;

        info!(%id, "task created as pending");
        Ok(id)
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

        let store = Arc::clone(self);
        let semaphore = self.semaphore.clone();

        tokio::spawn(async move {
            let _permit = match semaphore.acquire().await {
                Ok(p) => p,
                Err(_) => {
                    error!(%task_id, "semaphore closed");
                    return;
                }
            };

            // Update status to running
            if let Err(e) = store.update_status(task_id, "running").await {
                error!(%task_id, error = %e, "failed to set running status");
                return;
            }

            let trigger: TriggerReason = match serde_json::from_value(task.trigger_data.clone()) {
                Ok(t) => t,
                Err(e) => {
                    error!(%task_id, error = %e, "failed to deserialize trigger");
                    let _ = store.update_status(task_id, "failed").await;
                    return;
                }
            };

            info!(%task_id, "job starting");

            match run_job(
                trigger,
                task.git_url.clone(),
                task.project_path.clone(),
                task.default_branch.clone(),
                store.config.clone(),
                store.gitlab.clone(),
            )
            .await
            {
                Ok(claude_output) => {
                    info!(%task_id, "job completed");
                    if let Err(e) = store.save_result(task_id, &claude_output).await {
                        error!(%task_id, error = %e, "failed to save result");
                    }
                    let status = if claude_output.is_error { "failed" } else { "completed" };
                    let _ = store.finish_task(task_id, status).await;
                }
                Err(e) => {
                    error!(%task_id, error = %e, "job failed");
                    let _ = store.save_error_result(task_id, &e.to_string()).await;
                    let _ = store.finish_task(task_id, "failed").await;
                }
            }
        });

        Ok(())
    }

    async fn update_status(&self, task_id: Uuid, status: &str) -> Result<()> {
        let mut task: tasks::ActiveModel = tasks::Entity::find_by_id(task_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?
            .into();

        task.status = Set(status.to_string());
        if status == "running" {
            task.started_at = Set(Some(Utc::now().into()));
        }
        task.update(&self.db).await?;
        Ok(())
    }

    async fn finish_task(&self, task_id: Uuid, status: &str) -> Result<()> {
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
