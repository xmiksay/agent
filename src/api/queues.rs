//! HTTP surface for the queue catalog (`/api/queues`). A queue carries no
//! secrets, so the entity `Model` is returned verbatim — no separate view type.
//! Enqueuing/dequeuing a task is done via `PATCH /api/tasks/{id}` (the
//! `queue_id`/`priority` edits), not here.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use tracing::warn;
use uuid::Uuid;

use crate::AppState;
use crate::entity::queues;
use crate::queues::{NewQueue, UpdateQueue};

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<queues::Model>>, StatusCode> {
    let queues = state.queue_store.list().await.map_err(|e| {
        warn!(error = %e, "list queues failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(queues))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<queues::Model>, StatusCode> {
    let queue = state
        .queue_store
        .get(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(queue))
}

pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<NewQueue>,
) -> Result<(StatusCode, Json<queues::Model>), (StatusCode, String)> {
    let queue = state
        .queue_store
        .create(req)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(queue)))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateQueue>,
) -> Result<Json<queues::Model>, (StatusCode, String)> {
    let queue = state
        .queue_store
        .update(id, req)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(queue))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .queue_store
        .delete(id)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}
