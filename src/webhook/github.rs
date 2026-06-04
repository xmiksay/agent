//! Parse + verify GitHub webhooks → NormalizedEvent.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use tracing::{debug, info, warn};

use crate::AppState;
use crate::project::ProviderKind;
use crate::provider::github::payload::*;
use crate::webhook::dispatch::dispatch;
use crate::webhook::gitlab::WebhookResponse;
use crate::webhook::normalized::{
    EventKind, NoteTargetRef, NormalizedEvent, ProjectRef, ReviewState,
};
use crate::workspace::layout::slugify;

pub async fn handle(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<WebhookResponse>), StatusCode> {
    let service = state
        .git_service_store
        .get_by_slug(ProviderKind::Github, &slug)
        .await
        .map_err(|e| {
            warn!(error = %e, "git_service lookup failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    verify_signature(&service.webhook_secret, &headers, &body)
        .map_err(|()| StatusCode::UNAUTHORIZED)?;

    let event_type = headers
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?
        .to_string();

    let normalized = match parse(&event_type, &body) {
        Ok(Some(n)) => n,
        Ok(None) => {
            debug!(event_type, "github event ignored");
            return Ok((StatusCode::OK, Json(WebhookResponse { task_ids: vec![] })));
        }
        Err(e) => {
            warn!(error = %e, event_type, "failed to parse github payload");
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    info!(?event_type, "dispatching github event");

    let task_ids = dispatch(&state, &service, normalized).await.map_err(|e| {
        warn!(error = %e, "dispatch error");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok((StatusCode::CREATED, Json(WebhookResponse { task_ids })))
}

fn verify_signature(secret: &str, headers: &HeaderMap, body: &[u8]) -> Result<(), ()> {
    let header = headers
        .get("X-Hub-Signature-256")
        .and_then(|v| v.to_str().ok())
        .ok_or(())?;
    let provided = header.strip_prefix("sha256=").ok_or(())?;
    let provided_bytes = hex::decode(provided).map_err(|_| ())?;

    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).map_err(|_| ())?;
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
        ssh_url: repo.ssh_url.clone(),
        default_branch: repo.default_branch.clone(),
    }
}

pub fn parse(event_type: &str, body: &[u8]) -> anyhow::Result<Option<NormalizedEvent>> {
    Ok(match event_type {
        "issues" => {
            let ev: IssuesEvent = serde_json::from_slice(body)?;
            let assignees: Vec<String> =
                ev.issue.assignees.iter().map(|u| u.login.clone()).collect();
            let body = ev.issue.body.unwrap_or_default();
            let kind = match ev.action.as_str() {
                "assigned" | "opened" => EventKind::IssueAssigned {
                    iid: ev.issue.number,
                    assignees,
                    title: ev.issue.title,
                    body,
                    url: ev.issue.html_url,
                },
                "edited" | "labeled" => EventKind::IssueUpdated {
                    iid: ev.issue.number,
                    assignees,
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
            let kind = match ev.action.as_str() {
                "closed" => EventKind::PrClosed {
                    iid: ev.pull_request.number,
                    source_branch: ev.pull_request.head.branch,
                    url: ev.pull_request.html_url,
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
            let kind = EventKind::PrReviewSubmitted {
                iid: ev.pull_request.number,
                source_branch: ev.pull_request.head.branch,
                target_branch: ev.pull_request.base.branch,
                review_body: ev.review.body.unwrap_or_default(),
                state,
                url: ev.review.html_url,
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
            // GitHub uses the same payload for PR comments — distinguished by
            // the presence of `pull_request` field on the issue.
            let target = if ev.pull_request.is_some() {
                NoteTargetRef::PullRequest {
                    iid: ev.issue.number,
                    source_branch: String::new(), // not in this payload; runner will look up
                }
            } else {
                NoteTargetRef::Issue {
                    iid: ev.issue.number,
                    source_branch: None,
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
