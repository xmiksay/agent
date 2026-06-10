//! HTTP surface for the `/api/providers` catalog. The `api_key` is write-only
//! (like service tokens): never returned, only `has_api_key` is. `api_url` is not
//! a secret and is returned.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Serialize;
use tracing::warn;
use uuid::Uuid;

use crate::AppState;
use crate::agent::KNOWN_PROVIDER_KINDS;
use crate::models::{NewProvider, Provider, UpdateProvider};

/// A provider as the SPA sees it — secret-free. `has_api_key` reflects whether an
/// API-mode key is stored without revealing it; `api_url` is the optional
/// base-URL override; `kinds` lists the system-defined backend keys a provider
/// may use (so the form doesn't hardcode them).
#[derive(Serialize)]
pub struct ProviderView {
    pub id: Uuid,
    pub kind: String,
    pub name: String,
    pub has_api_key: bool,
    pub api_url: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl ProviderView {
    fn from(p: Provider) -> Self {
        Self {
            id: p.id,
            kind: p.kind,
            name: p.name,
            has_api_key: p.api_key.is_some(),
            api_url: p.api_url,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Serialize)]
pub struct ListResponse {
    pub providers: Vec<ProviderView>,
    /// The system-defined backend keys a provider's `kind` may be.
    pub kinds: Vec<&'static str>,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<ListResponse>, StatusCode> {
    let providers = state.provider_store.list().await.map_err(|e| {
        warn!(error = %e, "list providers failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(ListResponse {
        providers: providers.into_iter().map(ProviderView::from).collect(),
        kinds: KNOWN_PROVIDER_KINDS.to_vec(),
    }))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProviderView>, StatusCode> {
    let p = state
        .provider_store
        .get(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(ProviderView::from(p)))
}

pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<NewProvider>,
) -> Result<(StatusCode, Json<ProviderView>), (StatusCode, String)> {
    let p = state
        .provider_store
        .create(req)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(ProviderView::from(p))))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateProvider>,
) -> Result<Json<ProviderView>, (StatusCode, String)> {
    let p = state
        .provider_store
        .update(id, req)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(ProviderView::from(p)))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .provider_store
        .delete(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}
