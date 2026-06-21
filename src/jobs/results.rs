//! Persistence of per-run metrics into `task_sessions`. Sessions are 1:N per
//! task: turns within one agent run (same `session_id`) accumulate into a single
//! row, while a new run (fresh `session_id`) starts a new row.

use anyhow::{Context, Result};
use sea_orm::*;
use uuid::Uuid;

use crate::entity::task_sessions;
use crate::jobs::store::TaskStore;
use crate::jobs::types::ClaudeOutput;

impl TaskStore {
    /// Record the current run's metrics. Sessions are 1:N per task: turns within
    /// one agent run (same `session_id`) accumulate into a single row, while a new
    /// run (fresh `session_id`) starts a new row, so the history is preserved.
    pub(crate) async fn replace_result(&self, task_id: Uuid, output: &ClaudeOutput) -> Result<()> {
        if !output.session_id.is_empty()
            && let Some(existing) = task_sessions::Entity::find()
                .filter(task_sessions::Column::TaskId.eq(task_id))
                .filter(task_sessions::Column::SessionId.eq(output.session_id.clone()))
                .one(self.db())
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
                .update(self.db())
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
            .exec(self.db())
            .await
            .context("failed to insert task session")?;

        Ok(())
    }

    pub(crate) async fn save_error_result(&self, task_id: Uuid, error: &str) -> Result<()> {
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
            .exec(self.db())
            .await
            .context("failed to insert error session")?;

        Ok(())
    }
}
