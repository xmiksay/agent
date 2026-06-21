use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Serialize;
use tracing::warn;
use uuid::Uuid;

use crate::AppState;
use crate::project::ProviderKind;
use crate::provider::gitlab::token as gitlab_token;
use crate::service::{AuthKind, NewService, Service, TriggerConfig, TriggerMode, UpdateService};

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
    /// Per-trigger-type model mapping (`trigger_type → model_id`). Filled by the
    /// handlers (a separate query); empty when the service maps no types.
    pub models: std::collections::BTreeMap<String, uuid::Uuid>,
    /// Per-trigger-type gating overrides (`trigger_type → {enabled, mode, label}`).
    /// Filled by the handlers (a separate query); empty when the service uses the
    /// defaults for every type.
    pub triggers: std::collections::BTreeMap<String, TriggerConfig>,
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
            models: std::collections::BTreeMap::new(),
            triggers: std::collections::BTreeMap::new(),
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
    let mut views = Vec::with_capacity(services.len());
    for svc in services {
        let id = svc.id;
        let mut view = ServiceView::from(svc);
        view.models = state
            .service_store
            .trigger_models(id)
            .await
            .unwrap_or_default();
        view.triggers = state
            .service_store
            .trigger_configs(id)
            .await
            .unwrap_or_default();
        views.push(view);
    }
    Ok(Json(views))
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
    let mut view = ServiceView::from(svc);
    view.models = state
        .service_store
        .trigger_models(id)
        .await
        .unwrap_or_default();
    view.triggers = state
        .service_store
        .trigger_configs(id)
        .await
        .unwrap_or_default();
    Ok(Json(view))
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
    let id = svc.id;
    state
        .providers
        .refresh(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut view = ServiceView::from(svc);
    view.models = state
        .service_store
        .trigger_models(id)
        .await
        .unwrap_or_default();
    view.triggers = state
        .service_store
        .trigger_configs(id)
        .await
        .unwrap_or_default();
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
        .refresh(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut view = ServiceView::from(svc);
    view.models = state
        .service_store
        .trigger_models(id)
        .await
        .unwrap_or_default();
    view.triggers = state
        .service_store
        .trigger_configs(id)
        .await
        .unwrap_or_default();
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
