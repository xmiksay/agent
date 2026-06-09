use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::AppState;
use crate::project::ProviderKind;
use crate::provider::github;
use crate::provider::gitlab::token as gitlab_token;
use crate::service::{
    AuthKind, NewService, Service, ServiceCredentials, TriggerMode, UpdateService,
};

#[derive(Serialize)]
pub struct ServiceView {
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
    /// `assignee` | `label` | `both` — how issue events trigger the agent.
    pub trigger_mode: TriggerMode,
    /// Label name watched when `trigger_mode` includes labels.
    pub trigger_label: String,
    /// True once an App install has been recorded (non-empty `installation_id`).
    /// Lets the UI show install status without exposing the secret bundle.
    pub app_installed: bool,
    /// Non-secret metadata about a GitLab bot token minted via the provisioning
    /// flow (`None` for GitHub, or GitLab services whose token was pasted by
    /// hand). The token value itself is never returned.
    pub gitlab_token: Option<gitlab_token::TokenMeta>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Webhook URL operators paste into GitLab/GitHub. Built from request host.
    /// Token + webhook_secret are intentionally never returned.
    pub webhook_path: String,
    /// Set **only** on the create/update response when the secret was just
    /// auto-generated (the field was left blank and none was stored). Revealed
    /// once so the operator can paste it into a GitHub App's app-level webhook;
    /// never returned by list/get, preserving the write-only invariant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_webhook_secret: Option<String>,
}

/// A random webhook secret, used when the operator leaves the field blank. Two
/// v4 UUIDs (hex, no dashes) = 64 chars of CSPRNG-backed entropy.
fn generate_webhook_secret() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

impl ServiceView {
    pub(crate) fn from(svc: Service) -> Self {
        let webhook_path = format!("/webhook/{}/{}", svc.kind.as_str(), svc.slug);
        let app_installed = svc
            .app_credentials
            .as_ref()
            .and_then(|c| c.get("installation_id"))
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty());
        // GitLab provisioning stores its non-secret token metadata in the same
        // `app_credentials` bundle; surface it so the UI can show expiry/status.
        let gitlab_token = (svc.kind == ProviderKind::Gitlab)
            .then_some(svc.app_credentials.as_ref())
            .flatten()
            .and_then(|c| serde_json::from_value(c.clone()).ok());
        Self {
            id: svc.id,
            kind: svc.kind,
            slug: svc.slug,
            display_name: svc.display_name,
            base_url: svc.base_url,
            bot_username: svc.bot_username,
            autofire: svc.autofire,
            auth_kind: svc.auth_kind,
            trigger_mode: svc.trigger_mode,
            trigger_label: svc.trigger_label,
            app_installed,
            gitlab_token,
            created_at: svc.created_at,
            updated_at: svc.updated_at,
            webhook_path,
            generated_webhook_secret: None,
        }
    }
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<ServiceView>>, StatusCode> {
    let services = state.service_store.list().await.map_err(|e| {
        warn!(error = %e, "list services failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(services.into_iter().map(ServiceView::from).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ServiceView>, StatusCode> {
    let svc = state
        .service_store
        .get(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(ServiceView::from(svc)))
}

pub async fn create(
    State(state): State<AppState>,
    Json(mut req): Json<NewService>,
) -> Result<(StatusCode, Json<ServiceView>), (StatusCode, String)> {
    // Blank secret → generate one and reveal it once in the response.
    let generated = req.webhook_secret.trim().is_empty().then(|| {
        let s = generate_webhook_secret();
        req.webhook_secret = s.clone();
        s
    });
    let svc = state
        .service_store
        .create(req)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    state
        .providers
        .refresh(svc.id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut view = ServiceView::from(svc);
    view.generated_webhook_secret = generated;
    Ok((StatusCode::CREATED, Json(view)))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut req): Json<UpdateService>,
) -> Result<Json<ServiceView>, (StatusCode, String)> {
    // A blank/omitted secret means "keep" — unless none is stored yet, in which
    // case generate one and reveal it (mirrors create). A non-empty kept secret
    // is never revealed, preserving the write-only invariant.
    let blank = req
        .webhook_secret
        .as_deref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);
    let mut generated = None;
    if blank {
        let current = state
            .service_store
            .get(id)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .ok_or((StatusCode::NOT_FOUND, "git service not found".into()))?;
        if current.webhook_secret.trim().is_empty() {
            let s = generate_webhook_secret();
            req.webhook_secret = Some(s.clone());
            generated = Some(s);
        } else {
            // Don't push an empty string through the store's "if Some, set" path.
            req.webhook_secret = None;
        }
    }
    let svc = state
        .service_store
        .update(id, req)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    state
        .providers
        .refresh(svc.id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut view = ServiceView::from(svc);
    view.generated_webhook_secret = generated;
    Ok(Json(view))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .service_store
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_secret_is_64_hex_chars_and_random() {
        let a = generate_webhook_secret();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, generate_webhook_secret());
    }
}
