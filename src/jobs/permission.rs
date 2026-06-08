//! Handle one `can_use_tool` permission prompt from the agent's stdout control
//! protocol. This replaces the old PreToolUse `/internal/authcheck` hook: the
//! same allowlist + operator-approval policy, driven entirely over the
//! stream-json channel we already own.
//!
//! Policy (matching the old hook, whose matchers were exactly Bash +
//! AskUserQuestion):
//!  - any other tool → allow immediately (preserves autonomous edits/reads);
//!  - Bash matching the project allowlist → allow immediately, no auth_request;
//!  - Bash not matching, or AskUserQuestion → park on operator approval.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tracing::{info, warn};
use uuid::Uuid;

use crate::agent::{PermissionDecision, PermissionRequest};
use crate::auth::operations::{build_matcher, is_allowed};
use crate::auth::store::{AuthStatus, AuthStore};
use crate::auth::waiter::AuthWaiter;
use crate::jobs::hub::{EnvelopeKind, LiveSessions};
use crate::project::ProjectStore;

/// How long the operator has to resolve an approval before we deny by default.
const OPERATOR_TIMEOUT_SECS: u64 = 600;

pub async fn handle_permission(
    req: PermissionRequest,
    task_id: Uuid,
    project_id: Option<Uuid>,
    hub: LiveSessions,
    auth_store: Arc<AuthStore>,
    auth_waiter: AuthWaiter,
    project_store: Arc<ProjectStore>,
) {
    let is_question = req.tool_name == "AskUserQuestion";

    // Any tool other than Bash / AskUserQuestion runs autonomously — the old
    // "auto" permission mode let edits/reads through without prompting.
    if req.tool_name != "Bash" && !is_question {
        respond(
            &hub,
            task_id,
            &req.request_id,
            PermissionDecision::Allow {
                updated_input: req.input.clone(),
            },
        )
        .await;
        return;
    }

    // Bash on the project allowlist is allowed without bothering the operator.
    if req.tool_name == "Bash" {
        let command = req
            .input
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("");
        if command_allowed(project_id, command, &project_store).await {
            info!(%task_id, command, "command allowed by policy");
            respond(
                &hub,
                task_id,
                &req.request_id,
                PermissionDecision::Allow {
                    updated_input: req.input.clone(),
                },
            )
            .await;
            return;
        }
    }

    // Operator path: open an auth_request, surface it on the live stream, and
    // park until the operator resolves it (or we time out).
    let (requested_op, metadata, prompt) = if is_question {
        let questions = req.input.get("questions").cloned();
        let summary = questions
            .as_ref()
            .map(summarize_questions)
            .unwrap_or_else(|| "(empty AskUserQuestion payload)".to_string());
        let prompt = format!(
            "Claude is asking the operator a question:\n\n{summary}\n\n\
             Reply with the answer; \"Approve\" passes the reply back to Claude, \
             \"Deny\" lets Claude know you declined.",
        );
        let metadata = questions.map(|q| serde_json::json!({ "questions": q }));
        (summary, metadata, prompt)
    } else {
        let command = req
            .input
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        let prompt = format!(
            "Claude wants to run an operation that is not in this project's allowlist:\n\n\
             > {command}\n\nApprove with optional reply, or deny.",
        );
        (command, None, prompt)
    };

    let auth = match auth_store
        .create_pending(task_id, requested_op, prompt, metadata)
        .await
    {
        Ok(a) => a,
        Err(e) => {
            warn!(%task_id, error = %e, "failed to create auth_request; denying tool");
            respond(
                &hub,
                task_id,
                &req.request_id,
                PermissionDecision::Deny {
                    message: "Approval could not be recorded.".to_string(),
                },
            )
            .await;
            return;
        }
    };
    let notifier = auth_waiter.register(auth.id);

    if let Ok(payload) = serde_json::to_value(&auth) {
        hub.publish_aux(task_id, EnvelopeKind::AuthRequest, payload)
            .await;
    }
    info!(auth_id = %auth.id, %task_id, "awaiting operator approval");

    let wait = tokio::time::timeout(
        Duration::from_secs(OPERATOR_TIMEOUT_SECS),
        notifier.notified(),
    );
    let decision = match wait.await {
        Err(_) => {
            warn!(auth_id = %auth.id, "operator approval timed out");
            PermissionDecision::Deny {
                message: "Operator approval timed out.".to_string(),
            }
        }
        Ok(()) => {
            let resolved = auth_store.get(auth.id).await.ok().flatten();
            let approved = resolved
                .as_ref()
                .map(|r| matches!(r.status, AuthStatus::Approved))
                .unwrap_or(false);
            let reply = resolved.and_then(|r| r.operator_reply);
            map_decision(is_question, approved, reply, &req.input)
        }
    };
    respond(&hub, task_id, &req.request_id, decision).await;
}

/// Turn a resolved approval into a control response. AskUserQuestion has no
/// "allow" path — answering it is always a deny whose message the model reads as
/// the answer.
fn map_decision(
    is_question: bool,
    approved: bool,
    reply: Option<String>,
    input: &Value,
) -> PermissionDecision {
    if is_question {
        let message = reply.unwrap_or_else(|| {
            if approved {
                "Approved.".to_string()
            } else {
                "Operator declined to answer.".to_string()
            }
        });
        return PermissionDecision::Deny { message };
    }
    if approved {
        PermissionDecision::Allow {
            updated_input: input.clone(),
        }
    } else {
        PermissionDecision::Deny {
            message: reply.unwrap_or_else(|| "Operator denied this command.".to_string()),
        }
    }
}

async fn command_allowed(
    project_id: Option<Uuid>,
    command: &str,
    project_store: &ProjectStore,
) -> bool {
    let allowed_ops: Vec<String> = match project_id {
        Some(pid) => match project_store.get_project_by_id(pid).await {
            Ok(Some(p)) => p.allowed_operations,
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    match build_matcher(&allowed_ops) {
        Ok(m) => is_allowed(&m, command),
        Err(e) => {
            warn!(error = %e, "bad allowed_operations glob in project config");
            false
        }
    }
}

async fn respond(
    hub: &LiveSessions,
    task_id: Uuid,
    request_id: &str,
    decision: PermissionDecision,
) {
    if !hub.respond_permission(task_id, request_id, decision).await {
        warn!(%task_id, request_id, "no live session to answer permission request");
    }
}

/// Render the questions array as a readable fallback for the auth list, matching
/// the shape the old shell hook produced.
fn summarize_questions(questions: &Value) -> String {
    let Some(arr) = questions.as_array() else {
        return "(empty AskUserQuestion payload)".to_string();
    };
    let blocks: Vec<String> = arr
        .iter()
        .map(|q| {
            let question = q.get("question").and_then(|v| v.as_str()).unwrap_or("");
            let multi = if q
                .get("multiSelect")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                " (multi-select)"
            } else {
                ""
            };
            let options: Vec<String> = q
                .get("options")
                .and_then(|v| v.as_array())
                .map(|opts| {
                    opts.iter()
                        .map(|o| {
                            let label = o.get("label").and_then(|v| v.as_str()).unwrap_or("");
                            match o.get("description").and_then(|v| v.as_str()) {
                                Some(d) if !d.is_empty() => format!("  - {label} — {d}"),
                                _ => format!("  - {label}"),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            format!("Q: {question}{multi}\n{}", options.join("\n"))
        })
        .collect();
    if blocks.is_empty() {
        "(empty AskUserQuestion payload)".to_string()
    } else {
        blocks.join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_approve_allows_with_input() {
        let input = serde_json::json!({"command": "ls"});
        let d = map_decision(false, true, None, &input);
        assert_eq!(
            d,
            PermissionDecision::Allow {
                updated_input: input
            }
        );
    }

    #[test]
    fn bash_deny_uses_reply_then_fallback() {
        let input = serde_json::json!({"command": "rm -rf /"});
        assert_eq!(
            map_decision(false, false, Some("too dangerous".into()), &input),
            PermissionDecision::Deny {
                message: "too dangerous".into()
            }
        );
        assert_eq!(
            map_decision(false, false, None, &input),
            PermissionDecision::Deny {
                message: "Operator denied this command.".into()
            }
        );
    }

    #[test]
    fn question_approve_delivers_reply_as_answer() {
        let input = serde_json::json!({"questions": []});
        assert_eq!(
            map_decision(true, true, Some("use option B".into()), &input),
            PermissionDecision::Deny {
                message: "use option B".into()
            }
        );
        // Approved with no reply text still denies-with-message (the answer).
        assert_eq!(
            map_decision(true, true, None, &input),
            PermissionDecision::Deny {
                message: "Approved.".into()
            }
        );
    }

    #[test]
    fn question_deny_uses_decline_message() {
        let input = serde_json::json!({"questions": []});
        assert_eq!(
            map_decision(true, false, None, &input),
            PermissionDecision::Deny {
                message: "Operator declined to answer.".into()
            }
        );
        // An explicit reply still wins over the decline fallback.
        assert_eq!(
            map_decision(true, false, Some("go with A".into()), &input),
            PermissionDecision::Deny {
                message: "go with A".into()
            }
        );
    }

    #[test]
    fn summarize_questions_renders_labels_and_options() {
        let q = serde_json::json!([
            {
                "question": "Which DB?",
                "multiSelect": false,
                "options": [
                    { "label": "Postgres", "description": "default" },
                    { "label": "SQLite" }
                ]
            }
        ]);
        let s = summarize_questions(&q);
        assert!(s.contains("Q: Which DB?"));
        assert!(s.contains("  - Postgres — default"));
        assert!(s.contains("  - SQLite"));
    }
}
