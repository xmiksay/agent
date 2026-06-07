use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use tracing::warn;
use uuid::Uuid;

use crate::AppState;
use crate::git_service::{GitService, NewGitService, UpdateGitService};
use crate::project::ProviderKind;

#[derive(Serialize)]
pub struct GitServiceView {
    pub id: Uuid,
    pub kind: ProviderKind,
    pub slug: String,
    pub display_name: String,
    pub base_url: String,
    pub bot_username: String,
    pub autofire: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Webhook URL operators paste into GitLab/GitHub. Built from request host.
    /// Token + webhook_secret are intentionally never returned.
    pub webhook_path: String,
}

impl GitServiceView {
    fn from(svc: GitService) -> Self {
        let webhook_path = format!("/webhook/{}/{}", svc.kind.as_str(), svc.slug);
        Self {
            id: svc.id,
            kind: svc.kind,
            slug: svc.slug,
            display_name: svc.display_name,
            base_url: svc.base_url,
            bot_username: svc.bot_username,
            autofire: svc.autofire,
            created_at: svc.created_at,
            updated_at: svc.updated_at,
            webhook_path,
        }
    }
}

pub async fn list(
    State(state): State<AppState>,
) -> Result<Json<Vec<GitServiceView>>, StatusCode> {
    let services = state.git_service_store.list().await.map_err(|e| {
        warn!(error = %e, "list git_services failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(services.into_iter().map(GitServiceView::from).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<GitServiceView>, StatusCode> {
    let svc = state
        .git_service_store
        .get(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(GitServiceView::from(svc)))
}

pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<NewGitService>,
) -> Result<(StatusCode, Json<GitServiceView>), (StatusCode, String)> {
    let svc = state
        .git_service_store
        .create(req)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    state.providers.refresh(svc.id).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;
    Ok((StatusCode::CREATED, Json(GitServiceView::from(svc))))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateGitService>,
) -> Result<Json<GitServiceView>, (StatusCode, String)> {
    let svc = state
        .git_service_store
        .update(id, req)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    state.providers.refresh(svc.id).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;
    Ok(Json(GitServiceView::from(svc)))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .git_service_store
        .delete(id)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    state.providers.refresh(id).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;
    Ok(StatusCode::NO_CONTENT)
}
