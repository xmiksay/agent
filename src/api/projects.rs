use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::project::{BranchEntry, NewProjectConfig, ProjectConfig};
use crate::service::AuthKind;
use crate::workspace::layout::slugify;

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

    let service_id = project.service_id.ok_or((
        StatusCode::BAD_REQUEST,
        "project is not linked to a git service".into(),
    ))?;
    let service = state
        .service_store
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
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("webhook registration failed: {e}"),
            )
        })?;

    Ok(Json(RegisterWebhookResponse {
        status: "registered".into(),
        message: format!("Webhook registered on {}.", project.full_name),
        webhook_url: Some(webhook_url),
    }))
}

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    pub service_id: Uuid,
    pub full_name: String,
    #[serde(default)]
    pub default_branch: Option<String>,
    #[serde(default)]
    pub remote_url: Option<String>,
}

/// Manually register a project instead of waiting for its first webhook. The
/// provider, bot username and slug are derived from the chosen git service; the
/// remote URL is derived from the service's base URL when left blank. Idempotent
/// via `upsert_project`, so creating an already-known project just returns it.
pub async fn create_project(
    State(state): State<AppState>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<ProjectConfig>), (StatusCode, String)> {
    let full_name = req.full_name.trim();
    if full_name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "full_name is required".into()));
    }

    let service = state
        .service_store
        .get(req.service_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "git service not found".into()))?;

    let default_branch = req
        .default_branch
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .unwrap_or("main")
        .to_string();
    let remote_url = req
        .remote_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(String::from)
        .unwrap_or_else(|| derive_remote_url(&service.base_url, full_name));

    let (config, _created) = state
        .project_store
        .upsert_project(NewProjectConfig {
            provider: service.kind,
            service_id: service.id,
            project_slug: slugify(full_name),
            full_name: full_name.to_string(),
            remote_url,
            default_branch,
            my_username: service.bot_username.clone(),
        })
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(config)))
}

/// Build a default SSH remote (`git@host:owner/repo.git`) from a service's API
/// base URL when the operator supplies none. The transport normalizes either an
/// SSH or HTTPS remote to token-HTTPS, so the SSH form here is only a default.
fn derive_remote_url(base_url: &str, full_name: &str) -> String {
    let host = base_url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .trim_start_matches("api."); // api.github.com -> github.com
    format!("git@{host}:{full_name}.git")
}

#[cfg(test)]
mod tests {
    use super::derive_remote_url;

    #[test]
    fn derives_github_ssh_remote_from_api_base() {
        assert_eq!(
            derive_remote_url("https://api.github.com", "owner/repo"),
            "git@github.com:owner/repo.git"
        );
    }

    #[test]
    fn derives_self_hosted_gitlab_remote() {
        assert_eq!(
            derive_remote_url("https://git.f13cybertech.com", "grp/proj"),
            "git@git.f13cybertech.com:grp/proj.git"
        );
    }
}
