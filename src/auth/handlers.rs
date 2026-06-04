//! `POST /internal/authcheck` — called from the Claude Code PreToolUse hook
//! running on the same host. Returns whether the command is allowed and, if
//! the operator was prompted, the operator's free-text reply.

use std::net::IpAddr;
use std::time::Duration;

use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::AppState;
use crate::auth::operations::{build_matcher, is_allowed};
use crate::auth::store::AuthStatus;

const OPERATOR_TIMEOUT_SECS: u64 = 600;

#[derive(Deserialize)]
pub struct AuthCheckRequest {
    pub task_id: Uuid,
    pub command: String,
    #[serde(default)]
    pub tool: Option<String>,
    /// For `AskUserQuestion`: the raw `tool_input.questions` array. Stored as
    /// metadata so the frontend can render buttons / checkboxes.
    #[serde(default)]
    pub questions: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct AuthCheckResponse {
    pub allowed: bool,
    pub reply: Option<String>,
    pub reason: Option<String>,
}

pub async fn authcheck(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<AuthCheckRequest>,
) -> Result<Json<AuthCheckResponse>, StatusCode> {
    // Loopback only.
    let ip = addr.ip();
    if !is_loopback(ip) {
        warn!(%ip, "rejected non-loopback authcheck");
        return Err(StatusCode::FORBIDDEN);
    }

    let task = state
        .task_store
        .get_task(req.task_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(|(t, _)| t)
        .ok_or(StatusCode::NOT_FOUND)?;

    // Find the project's allowlist.
    let project_id = task.project_id;
    let allowed_ops: Vec<String> = match project_id {
        Some(pid) => state
            .project_store
            .get_project_by_id(pid)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .map(|p| p.allowed_operations)
            .unwrap_or_default(),
        None => Vec::new(),
    };

    let is_question = req.tool.as_deref() == Some("AskUserQuestion");

    if !is_question {
        let matcher = build_matcher(&allowed_ops).map_err(|e| {
            warn!(error = %e, "bad allowed_operations glob in project config");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        if is_allowed(&matcher, &req.command) {
            info!(task_id = %req.task_id, command = %req.command, "command allowed by policy");
            return Ok(Json(AuthCheckResponse {
                allowed: true,
                reply: None,
                reason: Some("matched allowlist".into()),
            }));
        }
    }

    // Open an auth request and wait for operator decision.
    let prompt = if is_question {
        format!(
            "Claude is asking the operator a question:\n\n{}\n\n\
             Reply with the answer; \"Approve\" passes the reply back to Claude, \
             \"Deny\" lets Claude know you declined.",
            req.command
        )
    } else {
        format!(
            "Claude wants to run an operation that is not in this project's allowlist:\n\n\
             > {}\n\nApprove with optional reply, or deny.",
            req.command
        )
    };
    let notifier = state.auth_waiter.register(Uuid::new_v4());
    drop(notifier); // we'll get the real id once the row is created

    let metadata = if is_question {
        req.questions.clone().map(|q| serde_json::json!({ "questions": q }))
    } else {
        None
    };

    let auth = state
        .auth_store
        .create_pending(req.task_id, req.command.clone(), prompt, metadata)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let notifier = state.auth_waiter.register(auth.id);

    info!(auth_id = %auth.id, task_id = %req.task_id, "awaiting operator approval");

    let wait = tokio::time::timeout(Duration::from_secs(OPERATOR_TIMEOUT_SECS), notifier.notified());
    match wait.await {
        Ok(()) => {
            let resolved = state
                .auth_store
                .get(auth.id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
            let allowed = matches!(resolved.status, AuthStatus::Approved);
            Ok(Json(AuthCheckResponse {
                allowed,
                reply: resolved.operator_reply,
                reason: Some(if allowed {
                    "operator approved".into()
                } else {
                    "operator denied".into()
                }),
            }))
        }
        Err(_) => {
            // Timeout: treat as denial but leave row pending for inspection.
            warn!(auth_id = %auth.id, "operator approval timed out");
            Ok(Json(AuthCheckResponse {
                allowed: false,
                reply: None,
                reason: Some("operator approval timed out".into()),
            }))
        }
    }
}

fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}
