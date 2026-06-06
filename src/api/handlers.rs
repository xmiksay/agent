use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::jobs::store::TaskEdits;
use crate::jobs::types::TriggerReason;

#[derive(Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
}

#[derive(Serialize)]
pub struct TaskDetail {
    #[serde(flatten)]
    pub task: crate::entity::tasks::Model,
    pub result: Option<crate::entity::task_results::Model>,
    /// Absolute path to this task's git worktree on the agent host.
    /// `None` if the task's git_service can no longer be resolved.
    pub work_dir: Option<String>,
    /// True when an agent process is attached and warm (idle between turns or
    /// actively running) — the SPA keeps its WebSocket open and chats live.
    pub live: bool,
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
    use crate::workspace::layout::slugify;

    let (task, result) = state
        .task_store
        .get_task(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let work_dir = match task.git_service_id {
        Some(sid) => state
            .task_store
            .providers()
            .service(sid)
            .await
            .map(|svc| {
                let project_slug = slugify(&task.project_path);
                let branch = task.branch.clone().unwrap_or_else(|| task.default_branch.clone());
                let branch_slug = slugify(&branch);
                state
                    .task_store
                    .workspace()
                    .branch_dir(&svc.slug, &project_slug, &branch_slug)
                    .to_string_lossy()
                    .into_owned()
            }),
        None => None,
    };

    let live = state.task_store.hub().is_warm(id).await;
    Ok(Json(TaskDetail { task, result, work_dir, live }))
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

#[derive(Deserialize)]
pub struct CreateTaskBody {
    pub project_id: Uuid,
    pub trigger: TriggerReason,
}

/// Operator-driven counterpart to the webhook dispatcher: pick a project,
/// hand over a fully-formed TriggerReason, get a pending task. Useful when
/// the webhook never arrived or was filtered out.
pub async fn create_task(
    State(state): State<AppState>,
    Json(payload): Json<CreateTaskBody>,
) -> Result<(StatusCode, Json<RetryResponse>), (StatusCode, String)> {
    let project = state
        .project_store
        .get_project_by_id(payload.project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "project not found".to_string()))?;

    let git_service_id = project.git_service_id.ok_or((
        StatusCode::BAD_REQUEST,
        "project has no git_service_id".to_string(),
    ))?;

    let id = state
        .task_store
        .create_task(
            payload.trigger,
            git_service_id,
            project.provider,
            Some(project.id),
            project.full_name,
            project.ssh_url,
            project.default_branch,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((StatusCode::CREATED, Json(RetryResponse { task_id: id })))
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

#[derive(Serialize)]
pub struct EventsResponse {
    /// Persisted agent events for the task, in order. The array index is the
    /// event `seq`, so the SPA can dedupe these against live WebSocket frames.
    pub events: Vec<serde_json::Value>,
}

/// Durable event history from `tasks.event_log`. Live events for an active task
/// arrive over the WebSocket; this seeds the timeline (and is the only source
/// once a task has finished and its in-memory session is gone).
pub async fn task_events(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<EventsResponse>, (StatusCode, String)> {
    let events = state
        .task_store
        .task_events(id)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(EventsResponse { events }))
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

pub async fn edit_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<TaskEdits>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .task_store
        .update_task(id, payload)
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
