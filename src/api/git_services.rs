use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::AppState;
use crate::git_service::{
    AuthKind, GitService, NewGitService, ServiceCredentials, UpdateGitService,
};
use crate::project::ProviderKind;
use crate::provider::github;

#[derive(Serialize)]
pub struct GitServiceView {
    pub id: Uuid,
    pub kind: ProviderKind,
    pub slug: String,
    pub display_name: String,
    pub base_url: String,
    pub bot_username: String,
    pub autofire: bool,
    /// `pat` or `app`. The `app_credentials` bundle is write-only and never
    /// returned, like `token`/`webhook_secret`.
    pub auth_kind: AuthKind,
    /// True once an App install has been recorded (non-empty `installation_id`).
    /// Lets the UI show install status without exposing the secret bundle.
    pub app_installed: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Webhook URL operators paste into GitLab/GitHub. Built from request host.
    /// Token + webhook_secret are intentionally never returned.
    pub webhook_path: String,
}

impl GitServiceView {
    fn from(svc: GitService) -> Self {
        let webhook_path = format!("/webhook/{}/{}", svc.kind.as_str(), svc.slug);
        let app_installed = svc
            .app_credentials
            .as_ref()
            .and_then(|c| c.get("installation_id"))
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty());
        Self {
            id: svc.id,
            kind: svc.kind,
            slug: svc.slug,
            display_name: svc.display_name,
            base_url: svc.base_url,
            bot_username: svc.bot_username,
            autofire: svc.autofire,
            auth_kind: svc.auth_kind,
            app_installed,
            created_at: svc.created_at,
            updated_at: svc.updated_at,
            webhook_path,
        }
    }
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<GitServiceView>>, StatusCode> {
    let services = state.git_service_store.list().await.map_err(|e| {
        warn!(error = %e, "list git_services failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(
        services.into_iter().map(GitServiceView::from).collect(),
    ))
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
    state
        .providers
        .refresh(svc.id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
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
    state
        .providers
        .refresh(svc.id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
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
    state
        .providers
        .refresh(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct InstallUrlView {
    /// Where the operator's browser should go to install the App. The SPA reads
    /// this and navigates — a bearer-gated endpoint can't itself be a top-level
    /// redirect the SPA could follow, so we hand back the URL instead of a 302.
    pub install_url: String,
}

/// `GET /api/git_services/{id}/github_app/install` — resolve the App's install
/// URL (with the service id round-tripped as `state`). Bearer-gated.
pub async fn github_app_install(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<InstallUrlView>, (StatusCode, String)> {
    let svc = state
        .git_service_store
        .get(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "git_service not found".to_string()))?;
    let cfg = match svc.credentials() {
        Ok(ServiceCredentials::GitHubApp(cfg)) => cfg,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "service is not a GitHub App (auth_kind must be 'app')".to_string(),
            ));
        }
    };
    let url = github::app::install_url(&cfg, &id.to_string())
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(InstallUrlView { install_url: url }))
}

#[derive(Deserialize)]
pub struct InstallCallback {
    /// The numeric installation id GitHub appends after a successful install.
    pub installation_id: Option<String>,
    /// Our service id, round-tripped from the install URL's `state`.
    pub state: Option<Uuid>,
}

/// `GET /github_app/callback` — GitHub redirects the operator's browser here
/// after they install the App. **Not** behind the bearer (GitHub's redirect
/// carries no token); the trust comes from `state` naming an existing app
/// service. Persists `installation_id` into that service's `app_credentials`,
/// then bounces back to the service page in the SPA.
pub async fn github_app_callback(
    State(state): State<AppState>,
    Query(cb): Query<InstallCallback>,
) -> Result<Redirect, (StatusCode, String)> {
    let (Some(service_id), Some(installation_id)) = (cb.state, cb.installation_id) else {
        return Err((
            StatusCode::BAD_REQUEST,
            "missing state or installation_id".to_string(),
        ));
    };

    let svc = state
        .git_service_store
        .get(service_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((
            StatusCode::BAD_REQUEST,
            "unknown service in state".to_string(),
        ))?;
    if svc.kind != ProviderKind::Github || svc.auth_kind != AuthKind::App {
        return Err((
            StatusCode::BAD_REQUEST,
            "service is not a GitHub App".to_string(),
        ));
    }

    let mut creds = svc
        .app_credentials
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    creds
        .as_object_mut()
        .ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "app_credentials is not an object".to_string(),
        ))?
        .insert(
            "installation_id".to_string(),
            installation_id.clone().into(),
        );

    state
        .git_service_store
        .update(
            service_id,
            UpdateGitService {
                app_credentials: Some(creds),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    state
        .providers
        .refresh(service_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    info!(%service_id, %installation_id, "GitHub App installed; persisted installation_id");

    Ok(Redirect::to(&format!("/git_services/{service_id}")))
}
