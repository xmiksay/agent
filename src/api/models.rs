//! HTTP surface for the model catalog (`/api/models`). A model carries no
//! secrets, so `AiModel` is returned verbatim — no separate view type.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use tracing::warn;
use uuid::Uuid;

use crate::AppState;
use crate::models::{AiModel, NewModel, UpdateModel};

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<AiModel>>, StatusCode> {
    let models = state.model_store.list().await.map_err(|e| {
        warn!(error = %e, "list models failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(models))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AiModel>, StatusCode> {
    let model = state
        .model_store
        .get(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(model))
}

pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<NewModel>,
) -> Result<(StatusCode, Json<AiModel>), (StatusCode, String)> {
    let model = state
        .model_store
        .create(req)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(model)))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateModel>,
) -> Result<Json<AiModel>, (StatusCode, String)> {
    let model = state
        .model_store
        .update(id, req)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(model))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .model_store
        .delete(id)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}
