//! Bearer-token gate for `/api/*` routes.
//!
//! If `API_BEARER_TOKEN` is unset (typical for dev), all requests pass.
//! Otherwise the `Authorization: Bearer <token>` header must match in
//! constant time. Webhooks, `/internal/authcheck`, `/health`, and the
//! SPA fallback are deliberately NOT covered — they have their own auth.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use subtle::ConstantTimeEq;

use crate::AppState;

pub async fn require_bearer(
    State(state): State<AppState>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(expected) = state.config.api_bearer_token.as_deref() else {
        return Ok(next.run(req).await);
    };

    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if expected.as_bytes().ct_eq(provided.as_bytes()).unwrap_u8() == 1 {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
