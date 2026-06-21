//! Parse + verify GitHub webhooks → NormalizedEvent.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use tracing::{info, warn};

use crate::AppState;
use crate::project::ProviderKind;
use crate::provider::BOT_NOTE_MARKER;
use crate::provider::github::payload::*;
use crate::webhook::dispatch::dispatch;
use crate::webhook::gitlab::WebhookResponse;
use crate::webhook::normalized::{
    EventKind, NormalizedEvent, NoteTargetRef, ProjectRef, ReviewState,
};
use crate::workspace::layout::slugify;

pub async fn handle(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<WebhookResponse>), StatusCode> {
    let service = state
        .service_store
        .get_by_slug(ProviderKind::Github, &slug)
        .await
        .map_err(|e| {
            warn!(error = %e, "service lookup failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let event_type = headers
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let action = parse_action(&body);
    let (hook_id, delivery, target) = source_headers(&headers);
    info!(slug = %slug, event = %event_type, action = %action, hook_id = %hook_id, delivery = %delivery, target = %target, "github webhook received");

    if verify_signature(&service.webhook_secret, &headers, &body).is_err() {
        // No secrets in the log — just the fact, the source, and the likely cause.
        // `target=integration` is the App's app-level webhook; `target=repository`
        // is a repo hook — so a mismatch points at exactly which one to re-secret.
        warn!(slug = %slug, event = %event_type, hook_id = %hook_id, target = %target, "github webhook REJECTED: X-Hub-Signature-256 mismatch (webhook secret differs from the service's)");
        return Err(StatusCode::UNAUTHORIZED);
    }
    if event_type.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let normalized = match parse(&event_type, &body) {
        Ok(Some(n)) => n,
        Ok(None) => {
            info!(slug = %slug, event = %event_type, action = %action, "github event ignored (no normalized form)");
            return Ok((StatusCode::OK, Json(WebhookResponse { task_ids: vec![] })));
        }
        Err(e) => {
            warn!(error = %e, event = %event_type, "failed to parse github payload");
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    let task_ids = dispatch(&state, &service, normalized).await.map_err(|e| {
        warn!(error = %e, "dispatch error");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    info!(slug = %slug, event = %event_type, action = %action, tasks = task_ids.len(), "github webhook handled");

    Ok((StatusCode::CREATED, Json(WebhookResponse { task_ids })))
}

/// GitHub delivery-identifying headers for logging — lets a log line tell a
/// repo-level hook (`target=repository`) apart from an App's app-level webhook
/// (`target=integration`) when both deliver to the same URL. No secrets here.
fn source_headers(headers: &HeaderMap) -> (String, String, String) {
    let h = |k: &str| {
        headers
            .get(k)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string()
    };
    (
        h("X-GitHub-Hook-ID"),
        h("X-GitHub-Delivery"),
        h("X-GitHub-Hook-Installation-Target-Type"),
    )
}

/// Best-effort `action` field for logging (`opened`, `labeled`, …); `-` if absent.
fn parse_action(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("action").and_then(|a| a.as_str()).map(String::from))
        .unwrap_or_else(|| "-".to_string())
}

fn verify_signature(secret: &str, headers: &HeaderMap, body: &[u8]) -> Result<(), ()> {
    let header = headers
        .get("X-Hub-Signature-256")
        .and_then(|v| v.to_str().ok())
        .ok_or(())?;
    let provided = header.strip_prefix("sha256=").ok_or(())?;
    let provided_bytes = hex::decode(provided).map_err(|_| ())?;

    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).map_err(|_| ())?;
    mac.update(body);
    let expected = mac.finalize().into_bytes();

    if expected.as_slice().ct_eq(&provided_bytes).unwrap_u8() == 1 {
        Ok(())
    } else {
        Err(())
    }
}

fn project_ref(repo: &Repository) -> ProjectRef {
    ProjectRef {
        full_name: repo.full_name.clone(),
        project_slug: slugify(&repo.full_name),
        remote_url: repo.ssh_url.clone(),
        default_branch: repo.default_branch.clone(),
    }
}

pub fn parse(event_type: &str, body: &[u8]) -> anyhow::Result<Option<NormalizedEvent>> {
    Ok(match event_type {
        "issues" => {
            let ev: IssuesEvent = serde_json::from_slice(body)?;
            let assignees: Vec<String> =
                ev.issue.assignees.iter().map(|u| u.login.clone()).collect();
            let labels: Vec<String> = ev.issue.labels.iter().map(|l| l.name.clone()).collect();
            let body = ev.issue.body.unwrap_or_default();
            let kind = match ev.action.as_str() {
                "assigned" | "opened" => EventKind::IssueAssigned {
                    iid: ev.issue.number,
                    assignees,
                    labels,
                    title: ev.issue.title,
                    body,
                    url: ev.issue.html_url,
                },
                "edited" | "labeled" => EventKind::IssueUpdated {
                    iid: ev.issue.number,
                    assignees,
                    labels,
                    title: ev.issue.title,
                    body,
                    url: ev.issue.html_url,
                },
                "closed" => EventKind::IssueClosed {
                    iid: ev.issue.number,
                    url: ev.issue.html_url,
                },
                _ => return Ok(None),
            };
            Some(NormalizedEvent {
                provider: ProviderKind::Github,
                project: project_ref(&ev.repository),
                actor: ev.sender.login,
                kind,
            })
        }
        "pull_request" => {
            let ev: PullRequestEvent = serde_json::from_slice(body)?;
            let reviewers: Vec<String> = ev
                .pull_request
                .requested_reviewers
                .iter()
                .map(|u| u.login.clone())
                .collect();
            let kind = match ev.action.as_str() {
                "closed" => EventKind::PrClosed {
                    iid: ev.pull_request.number,
                    source_branch: ev.pull_request.head.branch,
                    url: ev.pull_request.html_url,
                },
                "review_requested" | "opened" | "reopened" => EventKind::ReviewRequested {
                    iid: ev.pull_request.number,
                    source_branch: ev.pull_request.head.branch,
                    target_branch: ev.pull_request.base.branch,
                    url: ev.pull_request.html_url,
                    reviewers,
                    title: ev.pull_request.title,
                },
                _ => return Ok(None),
            };
            Some(NormalizedEvent {
                provider: ProviderKind::Github,
                project: project_ref(&ev.repository),
                actor: ev.sender.login,
                kind,
            })
        }
        "pull_request_review" => {
            let ev: PullRequestReviewEvent = serde_json::from_slice(body)?;
            if ev.action != "submitted" {
                return Ok(None);
            }
            let state = match ev.review.state.as_str() {
                "approved" => ReviewState::Approved,
                "changes_requested" => ReviewState::ChangesRequested,
                "commented" => ReviewState::Commented,
                _ => ReviewState::Other,
            };
            let reviewers: Vec<String> = ev
                .pull_request
                .requested_reviewers
                .iter()
                .map(|u| u.login.clone())
                .collect();
            let kind = EventKind::PrReviewSubmitted {
                iid: ev.pull_request.number,
                source_branch: ev.pull_request.head.branch,
                target_branch: ev.pull_request.base.branch,
                review_body: ev.review.body.unwrap_or_default(),
                state,
                url: ev.review.html_url,
                reviewers,
                author: Some(ev.pull_request.user.login),
            };
            Some(NormalizedEvent {
                provider: ProviderKind::Github,
                project: project_ref(&ev.repository),
                actor: ev.sender.login,
                kind,
            })
        }
        "issue_comment" => {
            let ev: IssueCommentEvent = serde_json::from_slice(body)?;
            if ev.action != "created" {
                return Ok(None);
            }
            let body = ev.comment.body.unwrap_or_default();
            // The bot stamps every comment it posts with BOT_NOTE_MARKER;
            // skip those so it doesn't react to its own posts.
            if body.contains(BOT_NOTE_MARKER) {
                return Ok(None);
            }
            let assignees: Vec<String> =
                ev.issue.assignees.iter().map(|u| u.login.clone()).collect();
            // GitHub uses the same payload for PR comments — distinguished by
            // the presence of `pull_request` field on the issue.
            let target = if ev.pull_request.is_some() {
                NoteTargetRef::PullRequest {
                    iid: ev.issue.number,
                    source_branch: String::new(), // not in this payload; runner will look up
                    author: None,                 // PR object isn't in this payload
                    reviewers: Vec::new(),
                }
            } else {
                NoteTargetRef::Issue {
                    iid: ev.issue.number,
                    source_branch: None,
                    assignees,
                }
            };
            Some(NormalizedEvent {
                provider: ProviderKind::Github,
                project: project_ref(&ev.repository),
                actor: ev.sender.login,
                kind: EventKind::NoteAdded {
                    target,
                    body,
                    url: ev.comment.html_url,
                },
            })
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Produce the `X-Hub-Signature-256` value GitHub would send for `body`
    /// under `secret`, using the same HMAC primitive as the verifier.
    fn sign(secret: &str, body: &[u8]) -> String {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    fn headers_with_sig(sig: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("X-Hub-Signature-256", sig.parse().unwrap());
        h
    }

    #[test]
    fn verify_signature_accepts_a_valid_signature() {
        let secret = "topsecret";
        let body = br#"{"action":"opened"}"#;
        let headers = headers_with_sig(&sign(secret, body));
        assert!(verify_signature(secret, &headers, body).is_ok());
    }

    #[test]
    fn verify_signature_rejects_a_wrong_secret() {
        let body = br#"{"action":"opened"}"#;
        // Signature computed under a different secret than the service holds.
        let headers = headers_with_sig(&sign("attacker", body));
        assert!(verify_signature("topsecret", &headers, body).is_err());
    }

    #[test]
    fn verify_signature_rejects_a_tampered_body() {
        let secret = "topsecret";
        let headers = headers_with_sig(&sign(secret, br#"{"action":"opened"}"#));
        // Same signature, body changed after signing.
        assert!(verify_signature(secret, &headers, br#"{"action":"closed"}"#).is_err());
    }

    #[test]
    fn verify_signature_rejects_a_missing_header() {
        assert!(verify_signature("topsecret", &HeaderMap::new(), b"{}").is_err());
    }

    #[test]
    fn verify_signature_rejects_a_header_without_the_sha256_prefix() {
        // Correct digest bytes but missing the `sha256=` scheme prefix.
        let raw = sign("topsecret", b"{}");
        let unprefixed = raw.strip_prefix("sha256=").unwrap().to_string();
        let headers = headers_with_sig(&unprefixed);
        assert!(verify_signature("topsecret", &headers, b"{}").is_err());
    }

    #[test]
    fn verify_signature_rejects_a_non_hex_digest() {
        let headers = headers_with_sig("sha256=not-hexadecimal-zzzz");
        assert!(verify_signature("topsecret", &headers, b"{}").is_err());
    }

    #[test]
    fn issue_labels_land_in_normalized_event() {
        let payload = serde_json::json!({
            "action": "labeled",
            "issue": {
                "number": 12,
                "title": "Add a thing",
                "body": "please",
                "html_url": "https://github.com/acme/repo/issues/12",
                "assignees": [],
                "labels": [{ "name": "agent" }, { "name": "bug" }]
            },
            "repository": {
                "full_name": "acme/repo",
                "ssh_url": "git@github.com:acme/repo.git",
                "default_branch": "main"
            },
            "sender": { "login": "operator" }
        })
        .to_string();
        let ev = parse("issues", payload.as_bytes()).unwrap().unwrap();
        match ev.kind {
            EventKind::IssueUpdated { labels, .. } => {
                assert_eq!(labels, vec!["agent".to_string(), "bug".to_string()]);
            }
            other => panic!("expected IssueUpdated, got {other:?}"),
        }
    }
}
