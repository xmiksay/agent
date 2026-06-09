//! GitLab bot-token self-service (#10): mint and rotate a Group/Project Access
//! Token from the SPA, so the operator never has to run `glab`. Provisioning
//! uses the service's current token as a one-shot owner-scoped bootstrap and
//! swaps it for the minted bot token; rotation reuses the bot token's own `api`
//! scope. Kept out of `api::services` to hold that file under the size cap.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use crate::AppState;
use crate::api::services::ServiceView;
use crate::project::ProviderKind;
use crate::provider::gitlab::token as gitlab_token;
use crate::service::{Service, ServiceCredentials, UpdateService};

type ApiError = (StatusCode, String);

#[derive(Deserialize)]
pub struct ProvisionTokenRequest {
    pub scope: gitlab_token::TokenScope,
    /// Group/project path (`my-group/sub`) or numeric id.
    pub namespace: String,
    /// Token name shown in GitLab. Defaults to `agent-{slug}`.
    #[serde(default)]
    pub name: Option<String>,
    /// `YYYY-MM-DD`. Defaults to ~364 days out (GitLab caps at 365).
    #[serde(default)]
    pub expires_at: Option<String>,
}

/// `POST /api/services/{id}/gitlab_token/provision` — mint a dedicated bot
/// Group/Project Access Token using the service's currently-stored token as an
/// owner-scoped bootstrap, then swap the stored token for the minted one. The
/// token value is never returned; rotation metadata is persisted for later.
pub async fn provision(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<ProvisionTokenRequest>,
) -> Result<Json<ServiceView>, ApiError> {
    let svc = load_gitlab_service(&state, id).await?;
    let bootstrap = pat_token(&svc)?;

    let params = gitlab_token::ProvisionParams {
        scope: req.scope,
        namespace: req.namespace.clone(),
        name: req
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| format!("agent-{}", svc.slug)),
        expires_at: req
            .expires_at
            .filter(|e| !e.trim().is_empty())
            .unwrap_or_else(gitlab_token::default_expiry),
    };

    let minted = gitlab_token::provision(&svc.base_url, &bootstrap, &params)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let meta = gitlab_token::TokenMeta {
        scope: req.scope,
        namespace: req.namespace,
        token_id: minted.token_id,
        expires_at: minted.expires_at,
    };
    info!(%id, token_id = meta.token_id, "provisioned GitLab bot access token");
    persist_token(&state, id, minted.token, &meta).await
}

/// `POST /api/services/{id}/gitlab_token/rotate` — rotate the previously
/// provisioned bot token in place (the current token authorizes its own
/// rotation via its `api` scope), persisting the new value and expiry.
pub async fn rotate(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ServiceView>, ApiError> {
    let svc = load_gitlab_service(&state, id).await?;
    let current = pat_token(&svc)?;
    let meta: gitlab_token::TokenMeta = svc
        .app_credentials
        .as_ref()
        .and_then(|c| serde_json::from_value(c.clone()).ok())
        .ok_or((
            StatusCode::BAD_REQUEST,
            "no provisioned token to rotate — provision one first".to_string(),
        ))?;

    let expires_at = gitlab_token::default_expiry();
    let minted = gitlab_token::rotate(&svc.base_url, &current, &meta, &expires_at)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let new_meta = gitlab_token::TokenMeta {
        token_id: minted.token_id,
        expires_at: minted.expires_at,
        ..meta
    };
    info!(%id, token_id = new_meta.token_id, "rotated GitLab bot access token");
    persist_token(&state, id, minted.token, &new_meta).await
}

async fn load_gitlab_service(state: &AppState, id: Uuid) -> Result<Service, ApiError> {
    let svc = state
        .service_store
        .get(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "service not found".to_string()))?;
    if svc.kind != ProviderKind::Gitlab {
        return Err((
            StatusCode::BAD_REQUEST,
            "access-token provisioning is GitLab-only".to_string(),
        ));
    }
    Ok(svc)
}

fn pat_token(svc: &Service) -> Result<String, ApiError> {
    match svc.credentials() {
        Ok(ServiceCredentials::Pat(t)) if !t.trim().is_empty() => Ok(t),
        Ok(ServiceCredentials::Pat(_)) => Err((
            StatusCode::BAD_REQUEST,
            "set an owner-scoped token on the service first (used to mint the bot token)"
                .to_string(),
        )),
        _ => Err((
            StatusCode::BAD_REQUEST,
            "service has no usable pat token".to_string(),
        )),
    }
}

async fn persist_token(
    state: &AppState,
    id: Uuid,
    token: String,
    meta: &gitlab_token::TokenMeta,
) -> Result<Json<ServiceView>, ApiError> {
    let app_credentials = serde_json::to_value(meta)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let svc = state
        .service_store
        .update(
            id,
            UpdateService {
                token: Some(token),
                app_credentials: Some(app_credentials),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    state
        .providers
        .refresh(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ServiceView::from(svc)))
}
