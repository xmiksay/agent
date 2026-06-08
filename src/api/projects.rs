use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::git_service::AuthKind;
use crate::project::{BranchEntry, ProjectConfig};

#[derive(Serialize)]
pub struct ProjectListItem {
    #[serde(flatten)]
    pub config: ProjectConfig,
    pub branch_count: usize,
}

#[derive(Serialize)]
pub struct ProjectDetailResponse {
    #[serde(flatten)]
    pub config: ProjectConfig,
    pub branches: Vec<BranchEntry>,
}

#[derive(Deserialize)]
pub struct UpdateConfigRequest {
    pub allowed_operations: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateEnvRequest {
    pub env_file: String,
}

pub async fn list_projects(
    State(state): State<AppState>,
) -> Result<Json<Vec<ProjectListItem>>, StatusCode> {
    let projects = state
        .project_store
        .list_projects()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut out = Vec::with_capacity(projects.len());
    for p in projects {
        let branches = state
            .project_store
            .list_branches(p.id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        out.push(ProjectListItem {
            branch_count: branches.len(),
            config: p,
        });
    }
    Ok(Json(out))
}

pub async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProjectDetailResponse>, StatusCode> {
    let config = state
        .project_store
        .get_project_by_id(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let branches = state
        .project_store
        .list_branches(config.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ProjectDetailResponse { config, branches }))
}

pub async fn list_branches(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<BranchEntry>>, StatusCode> {
    let project = state
        .project_store
        .get_project_by_id(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let branches = state
        .project_store
        .list_branches(project.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(branches))
}

pub async fn update_config(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<Json<ProjectConfig>, StatusCode> {
    let project = state
        .project_store
        .get_project_by_id(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let updated = state
        .project_store
        .update_allowed_ops(project.id, req.allowed_operations)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(updated))
}

pub async fn update_env(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateEnvRequest>,
) -> Result<Json<ProjectConfig>, StatusCode> {
    let project = state
        .project_store
        .get_project_by_id(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let updated = state
        .project_store
        .update_env_file(project.id, req.env_file)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(updated))
}

#[derive(Serialize)]
pub struct RegisterWebhookResponse {
    /// `registered` | `skipped` — `skipped` for App services (app-level hook).
    pub status: String,
    pub message: String,
    pub webhook_url: Option<String>,
}

/// Idempotently register this project's inbound webhook on the provider. Lets the
/// operator wire an *existing* repo that predates the agent — auto-registration
/// only fires on a project's first inbound event, so a repo that has never sent
/// one can't bootstrap itself. Errors carry the provider's message so the SPA can
/// show why (e.g. insufficient token scope).
pub async fn register_webhook(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RegisterWebhookResponse>, (StatusCode, String)> {
    let project = state
        .project_store
        .get_project_by_id(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "project not found".into()))?;

    let service_id = project.git_service_id.ok_or((
        StatusCode::BAD_REQUEST,
        "project is not linked to a git service".into(),
    ))?;
    let service = state
        .git_service_store
        .get(service_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "git service not found".into()))?;

    // App services receive events through the App's single app-level webhook;
    // there is nothing to register per-repo (and the App token can't anyway).
    if service.auth_kind == AuthKind::App {
        return Ok(Json(RegisterWebhookResponse {
            status: "skipped".into(),
            message: "App service: events arrive via the App's app-level webhook — no per-repo hook to register.".into(),
            webhook_url: None,
        }));
    }

    let base = state.config.public_base_url.clone().ok_or((
        StatusCode::BAD_REQUEST,
        "PUBLIC_BASE_URL is not set — cannot build the webhook callback URL".into(),
    ))?;
    let webhook_url = format!("{base}/webhook/{}/{}", service.kind.as_str(), service.slug);

    let provider = state.providers.get(service_id).await.ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "provider client not loaded for this service".into(),
    ))?;
    provider
        .ensure_webhook(&project.full_name, &webhook_url, &service.webhook_secret)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("webhook registration failed: {e}")))?;

    Ok(Json(RegisterWebhookResponse {
        status: "registered".into(),
        message: format!("Webhook registered on {}.", project.full_name),
        webhook_url: Some(webhook_url),
    }))
}
