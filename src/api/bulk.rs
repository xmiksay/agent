//! Bulk task actions for the SPA's multi-select toolbar. One endpoint applies a
//! single lifecycle action across many tasks, reusing the per-task `TaskStore`
//! methods and reporting a per-id success/failure split so a few bad rows don't
//! sink the whole batch.

use std::sync::Arc;

use anyhow::Result;
use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::jobs::lifecycle::TASK_PENDING;
use crate::jobs::store::TaskStore;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkAction {
    /// Start a pending task (confirm) or resume a paused one (continue), picked
    /// per task from its current state.
    Run,
    /// Pause: SIGKILL the live agent but keep the session for a later resume.
    Pause,
    /// Resume a paused/failed task using its captured session.
    Resume,
    /// Delete the task row (force-kills a live agent first).
    Delete,
}

#[derive(Deserialize)]
pub struct BulkActionBody {
    pub action: BulkAction,
    pub ids: Vec<Uuid>,
}

#[derive(Serialize)]
pub struct BulkFailure {
    pub id: Uuid,
    pub error: String,
}

#[derive(Serialize)]
pub struct BulkActionResponse {
    pub succeeded: Vec<Uuid>,
    pub failed: Vec<BulkFailure>,
}

pub async fn bulk_action(
    State(state): State<AppState>,
    Json(body): Json<BulkActionBody>,
) -> Json<BulkActionResponse> {
    let store = &state.task_store;
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    // Sequential by design: at single-operator scale the batches are small, and
    // serializing keeps the per-branch worktree locks contention-free.
    for id in body.ids {
        let result = match body.action {
            BulkAction::Run => run_one(store, id).await,
            BulkAction::Pause => store.kill_task(id).await,
            BulkAction::Resume => store.continue_task(id).await.map(|_| ()),
            BulkAction::Delete => store.delete_task(id).await,
        };
        match result {
            Ok(()) => succeeded.push(id),
            Err(e) => failed.push(BulkFailure {
                id,
                error: e.to_string(),
            }),
        }
    }

    Json(BulkActionResponse { succeeded, failed })
}

/// "Run" means different things by state: a never-started task is confirmed, an
/// already-run one is resumed from its session. Mirrors the push_message cold path.
async fn run_one(store: &Arc<TaskStore>, id: Uuid) -> Result<()> {
    let (task, _) = store
        .get_task(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("task not found"))?;
    if task.task_state == TASK_PENDING {
        store.confirm_task(id).await
    } else {
        store.continue_task(id).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SPA sends `action` as a snake_case string and a flat `ids` array —
    /// guard that contract.
    #[test]
    fn parses_snake_case_action_and_ids() {
        let id = Uuid::new_v4();
        let body: BulkActionBody =
            serde_json::from_value(serde_json::json!({ "action": "delete", "ids": [id] })).unwrap();
        assert!(matches!(body.action, BulkAction::Delete));
        assert_eq!(body.ids, vec![id]);
    }

    /// A per-id failure must serialize with `id` + `error` so the SPA can report
    /// which rows didn't take.
    #[test]
    fn failure_serializes_id_and_error() {
        let id = Uuid::new_v4();
        let resp = BulkActionResponse {
            succeeded: vec![],
            failed: vec![BulkFailure {
                id,
                error: "task not found".to_string(),
            }],
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["failed"][0]["id"], id.to_string());
        assert_eq!(v["failed"][0]["error"], "task not found");
        assert!(v["succeeded"].as_array().unwrap().is_empty());
    }
}
