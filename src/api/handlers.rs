use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;

#[derive(Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
}

#[derive(Serialize)]
pub struct TaskDetail {
    #[serde(flatten)]
    pub task: crate::entity::tasks::Model,
    pub result: Option<crate::entity::task_results::Model>,
}

pub async fn list_tasks(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<crate::entity::tasks::Model>>, StatusCode> {
    let tasks = state
        .task_store
        .list_tasks(query.status.as_deref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(tasks))
}

pub async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskDetail>, StatusCode> {
    let result = state
        .task_store
        .get_task(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(TaskDetail {
        task: result.0,
        result: result.1,
    }))
}

pub async fn confirm_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .task_store
        .confirm_task(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(StatusCode::ACCEPTED)
}
