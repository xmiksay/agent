use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::auth::resolve::resolve_and_publish;
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
    /// Structured AskUserQuestion answers (`{ "<question>": "<label|custom>" | ["<label>", …] }`).
    /// When present we stringify it into `operator_reply`, which the parked
    /// question handler parses back out — operator_reply doubles as the answers
    /// carrier so a question approval rides the normal resolve path.
    #[serde(default)]
    pub answers: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct ResolveResponse {
    #[serde(flatten)]
    pub request: AuthRequest,
}

#[derive(Deserialize)]
pub struct BulkResolveRequest {
    /// Explicit ids to resolve. Ignored when `all_pending` is set.
    #[serde(default)]
    pub ids: Vec<Uuid>,
    /// Target every currently-pending request instead of `ids`.
    #[serde(default)]
    pub all_pending: bool,
    pub decision: String, // "approve" | "deny"
    #[serde(default)]
    pub reply: Option<String>,
}

#[derive(Serialize)]
pub struct BulkResolveResponse {
    pub resolved: usize,
}

/// Map the wire decision string to a terminal status. `pending` is not a valid
/// resolution target.
fn parse_decision(s: &str) -> Result<AuthStatus, (StatusCode, String)> {
    match s {
        "approve" => Ok(AuthStatus::Approved),
        "deny" => Ok(AuthStatus::Denied),
        other => Err((
            StatusCode::BAD_REQUEST,
            format!("invalid decision '{other}'"),
        )),
    }
}

/// Pick the `operator_reply` to persist. Structured AskUserQuestion `answers`
/// win and are stringified into the reply column (the question handler parses
/// them back); otherwise the freeform `reply` text is used as-is.
fn resolve_reply(answers: Option<serde_json::Value>, reply: Option<String>) -> Option<String> {
    match answers {
        Some(answers) => Some(answers.to_string()),
        None => reply,
    }
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
    let decision = parse_decision(&req.decision)?;
    let reply = resolve_reply(req.answers, req.reply);
    let resolved = resolve_and_publish(
        &state.auth_store,
        &state.auth_waiter,
        state.task_store.hub(),
        id,
        decision,
        reply,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ResolveResponse { request: resolved }))
}

/// Resolve many pending approvals at once (the operator's "deny all" escape from
/// a clogged queue). Targets `all_pending` rows or the explicit `ids`, but only
/// rows still `pending` — already-resolved ones are skipped, so a retry is
/// idempotent. Returns how many rows this call resolved.
pub async fn bulk_resolve_auth_requests(
    State(state): State<AppState>,
    Json(req): Json<BulkResolveRequest>,
) -> Result<Json<BulkResolveResponse>, (StatusCode, String)> {
    let decision = parse_decision(&req.decision)?;

    let internal = |e: anyhow::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());

    let targets: Vec<Uuid> = if req.all_pending {
        state
            .auth_store
            .list_filtered(Some(AuthStatus::Pending), None)
            .await
            .map_err(internal)?
            .into_iter()
            .map(|r| r.id)
            .collect()
    } else {
        // Keep only ids that are still pending, so resolving twice can't clobber
        // an already-recorded decision (idempotence).
        let mut pending = Vec::new();
        for id in req.ids {
            if let Some(r) = state.auth_store.get(id).await.map_err(internal)?
                && matches!(r.status, AuthStatus::Pending)
            {
                pending.push(id);
            }
        }
        pending
    };

    let mut resolved = 0usize;
    for id in targets {
        match resolve_and_publish(
            &state.auth_store,
            &state.auth_waiter,
            state.task_store.hub(),
            id,
            decision,
            req.reply.clone(),
        )
        .await
        {
            Ok(_) => resolved += 1,
            Err(e) => tracing::warn!(%id, error = %e, "bulk resolve: skipping row"),
        }
    }
    Ok(Json(BulkResolveResponse { resolved }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_stringified_into_reply() {
        let answers = serde_json::json!({
            "Which DB?": "Postgres",
            "Which caches?": ["Redis", "Memcached"],
        });
        let stored = resolve_reply(Some(answers.clone()), Some("ignored".into()))
            .expect("answers produce a reply");
        // The stored string round-trips back to the original answers object —
        // this is exactly what the parked question handler parses out.
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stored).unwrap(),
            answers
        );
    }

    #[test]
    fn freeform_reply_used_when_no_answers() {
        assert_eq!(
            resolve_reply(None, Some("looks fine".into())),
            Some("looks fine".to_string())
        );
        assert_eq!(resolve_reply(None, None), None);
    }
}
