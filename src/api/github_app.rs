//! GitHub App setup endpoints — install-URL resolution, the post-install
//! callback, and the JWT-driven self-sync. Split out of `api/services.rs` to keep
//! it under the file cap; the service CRUD + `ServiceView` stay there.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::AppState;
use crate::project::ProviderKind;
use crate::provider::github;
use crate::service::{AuthKind, ServiceCredentials, UpdateService};

#[derive(Serialize)]
pub struct InstallUrlView {
    /// Where the operator's browser should go to install the App. The SPA reads
    /// this and navigates — a bearer-gated endpoint can't itself be a top-level
    /// redirect the SPA could follow, so we hand back the URL instead of a 302.
    pub install_url: String,
}

/// `GET /api/services/{id}/github_app/install` — resolve the App's install
/// URL (with the service id round-tripped as `state`). Bearer-gated.
pub async fn github_app_install(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<InstallUrlView>, (StatusCode, String)> {
    let svc = state
        .service_store
        .get(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "service not found".to_string()))?;
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
        .service_store
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
        .service_store
        .update(
            service_id,
            UpdateService {
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

    Ok(Redirect::to(&format!("/services/{service_id}")))
}

#[derive(Serialize)]
pub struct GitHubAppSyncResult {
    pub installation_id: String,
    pub webhook_registered: bool,
    pub webhook_url: Option<String>,
    pub message: String,
}

/// Let the bot finish App setup itself, using the App JWT: discover + persist the
/// installation id (`GET /app/installations`, no redirect needed) and register
/// the app-level webhook (`PATCH /app/hook/config`). Idempotent — safe to re-run.
pub async fn github_app_sync(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<GitHubAppSyncResult>, (StatusCode, String)> {
    let svc = state
        .service_store
        .get(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "service not found".to_string()))?;
    let cfg = match svc.credentials() {
        Ok(ServiceCredentials::GitHubApp(cfg)) => cfg,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "service is not a GitHub App (auth_kind must be 'app')".to_string(),
            ));
        }
    };

    // 1. Discover the installation and persist it (mirrors the callback).
    let installation_id = github::app::discover_installation_id(&cfg)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
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
        .service_store
        .update(
            id,
            UpdateService {
                app_credentials: Some(creds),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    // 2. Register the app-level webhook, if we have a public base URL to point at.
    let (webhook_registered, webhook_url) = match state.config.public_base_url.clone() {
        Some(base) => {
            let url = format!("{base}/webhook/{}/{}", svc.kind.as_str(), svc.slug);
            github::app::set_app_webhook(&cfg, &url, &svc.webhook_secret)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
            (true, Some(url))
        }
        None => (false, None),
    };

    state
        .providers
        .refresh(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let message = if webhook_registered {
        format!(
            "Installed (installation {installation_id}); app-level webhook registered. Make sure the App is subscribed to Issues / Pull request / comment events."
        )
    } else {
        format!(
            "Installed (installation {installation_id}). Set PUBLIC_BASE_URL so the webhook can be registered automatically too."
        )
    };
    info!(%id, %installation_id, webhook_registered, "GitHub App synced");
    Ok(Json(GitHubAppSyncResult {
        installation_id,
        webhook_registered,
        webhook_url,
        message,
    }))
}
