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

#[derive(Serialize)]
pub struct RetryResponse {
    pub task_id: Uuid,
}

pub async fn retry_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RetryResponse>, (StatusCode, String)> {
    let new_id = state
        .task_store
        .retry_task(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(RetryResponse { task_id: new_id }))
}

pub async fn kill_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .task_store
        .kill_task(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn task_output(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<crate::jobs::output_log::TaskOutput>, StatusCode> {
    state
        .task_store
        .output_log()
        .get(id)
        .await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .task_store
        .delete_task(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn continue_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RetryResponse>, (StatusCode, String)> {
    let new_id = state
        .task_store
        .continue_task(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(RetryResponse { task_id: new_id }))
}

#[derive(Deserialize)]
pub struct PushMessageBody {
    pub body: String,
}

pub async fn push_message(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<PushMessageBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .task_store
        .push_message(id, payload.body)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Serialize)]
pub struct DiffResponse {
    pub diff: String,
}

pub async fn task_diff(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<DiffResponse>, (StatusCode, String)> {
    let diff = state
        .task_store
        .branch_diff(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(DiffResponse { diff }))
}
