use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::auth::store::{AuthRequest, AuthStatus};

#[derive(Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
    pub task_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct ResolveRequest {
    pub decision: String, // "approve" | "deny"
    #[serde(default)]
    pub reply: Option<String>,
}

#[derive(Serialize)]
pub struct ResolveResponse {
    #[serde(flatten)]
    pub request: AuthRequest,
}

pub async fn list_auth_requests(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<AuthRequest>>, StatusCode> {
    let status = match q.status.as_deref() {
        Some(s) => Some(AuthStatus::parse(s).map_err(|_| StatusCode::BAD_REQUEST)?),
        None => None,
    };
    let rows = state
        .auth_store
        .list_filtered(status, q.task_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows))
}

pub async fn get_auth_request(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AuthRequest>, StatusCode> {
    let r = state
        .auth_store
        .get(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(r))
}

pub async fn resolve_auth_request(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<ResolveRequest>,
) -> Result<Json<ResolveResponse>, (StatusCode, String)> {
    let decision = match req.decision.as_str() {
        "approve" => AuthStatus::Approved,
        "deny" => AuthStatus::Denied,
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("invalid decision '{other}'"),
            ));
        }
    };
    let resolved = state
        .auth_store
        .resolve(id, decision, req.reply)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state.auth_waiter.notify(id);
    // Push the resolution to the task's live stream so the inline approval card
    // clears without waiting for the next poll.
    if let Ok(payload) = serde_json::to_value(&resolved) {
        state
            .task_store
            .hub()
            .publish_aux(
                resolved.task_id,
                crate::jobs::hub::EnvelopeKind::AuthRequest,
                payload,
            )
            .await;
    }
    Ok(Json(ResolveResponse { request: resolved }))
}
