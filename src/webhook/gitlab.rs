//! Parse + verify GitLab webhooks → NormalizedEvent.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Serialize;
use subtle::ConstantTimeEq;
use tracing::{debug, info, warn};

use crate::AppState;
use crate::project::ProviderKind;
use crate::provider::BOT_NOTE_MARKER;
use crate::webhook::dispatch::dispatch;
use crate::webhook::normalized::{
    EventKind, NormalizedEvent, NoteTargetRef, ProjectRef, ReviewState,
};
use crate::webhook::types::*;

use crate::workspace::layout::slugify;

#[derive(Serialize)]
pub struct WebhookResponse {
    pub task_ids: Vec<uuid::Uuid>,
}

/// Verify `X-Gitlab-Token` against per-service `webhook_secret`, then dispatch.
pub async fn handle(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(event): Json<GitLabEvent>,
) -> Result<(StatusCode, Json<WebhookResponse>), StatusCode> {
    let service = state
        .git_service_store
        .get_by_slug(ProviderKind::Gitlab, &slug)
        .await
        .map_err(|e| {
            warn!(error = %e, "git_service lookup failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let expected = service.webhook_secret.as_bytes();
    let actual = headers
        .get("X-Gitlab-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?
        .as_bytes();
    if expected.ct_eq(actual).unwrap_u8() != 1 {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let Some(normalized) = normalize(&event) else {
        debug!("event ignored: no normalized form");
        return Ok((StatusCode::OK, Json(WebhookResponse { task_ids: vec![] })));
    };
    info!(kind = ?std::mem::discriminant(&normalized.kind), "dispatching gitlab event");

    let task_ids = dispatch(&state, &service, normalized).await.map_err(|e| {
        warn!(error = %e, "dispatch error");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok((StatusCode::CREATED, Json(WebhookResponse { task_ids })))
}

pub fn normalize(event: &GitLabEvent) -> Option<NormalizedEvent> {
    match event {
        GitLabEvent::Issue(e) => normalize_issue(e),
        GitLabEvent::MergeRequest(e) => normalize_merge_request(e),
        GitLabEvent::Note(e) => normalize_note(e),
    }
}

fn project_ref(p: &Project) -> ProjectRef {
    ProjectRef {
        full_name: p.path_with_namespace.clone(),
        project_slug: slugify(&p.path_with_namespace),
        ssh_url: p.git_ssh_url.clone(),
        default_branch: p.default_branch.clone(),
    }
}

fn normalize_issue(e: &IssueEvent) -> Option<NormalizedEvent> {
    let attrs = &e.object_attributes;
    let action = attrs.action.as_deref()?;
    let assignees: Vec<String> = e.assignees.iter().map(|a| a.username.clone()).collect();
    let kind = match action {
        "open" => EventKind::IssueAssigned {
            iid: attrs.iid,
            assignees,
            title: attrs.title.clone(),
            body: attrs.description.clone().unwrap_or_default(),
            url: attrs.url.clone(),
        },
        "update" => EventKind::IssueUpdated {
            iid: attrs.iid,
            assignees,
            title: attrs.title.clone(),
            body: attrs.description.clone().unwrap_or_default(),
            url: attrs.url.clone(),
        },
        "close" => EventKind::IssueClosed {
            iid: attrs.iid,
            url: attrs.url.clone(),
        },
        _ => return None,
    };
    Some(NormalizedEvent {
        provider: ProviderKind::Gitlab,
        project: project_ref(&e.project),
        actor: e.user.username.clone(),
        kind,
    })
}

fn normalize_merge_request(e: &MergeRequestEvent) -> Option<NormalizedEvent> {
    let attrs = &e.object_attributes;
    let action = attrs.action.as_deref()?;
    let reviewers: Vec<String> = e.reviewers.iter().map(|r| r.username.clone()).collect();
    let kind = match action {
        "approved" | "unapproved" | "approval" | "unapproval" => EventKind::PrReviewSubmitted {
            iid: attrs.iid,
            source_branch: attrs.source_branch.clone(),
            target_branch: attrs.target_branch.clone(),
            review_body: String::new(),
            state: match action {
                "approved" | "approval" => ReviewState::Approved,
                // "unapproved" fires when an MR drops below approval threshold
                // (e.g. "Request changes"); "unapproval" fires per-reviewer.
                _ => ReviewState::ChangesRequested,
            },
            url: attrs.url.clone(),
            reviewers,
            // GitLab webhook only gives author_id (numeric), not username.
            author: None,
        },
        "open" | "update" | "reopen" => EventKind::ReviewRequested {
            iid: attrs.iid,
            source_branch: attrs.source_branch.clone(),
            target_branch: attrs.target_branch.clone(),
            url: attrs.url.clone(),
            reviewers,
            title: attrs.title.clone(),
        },
        "close" | "merge" => EventKind::PrClosed {
            iid: attrs.iid,
            source_branch: attrs.source_branch.clone(),
            url: attrs.url.clone(),
        },
        _ => return None,
    };
    Some(NormalizedEvent {
        provider: ProviderKind::Gitlab,
        project: project_ref(&e.project),
        actor: e.user.username.clone(),
        kind,
    })
}

fn normalize_note(e: &NoteEvent) -> Option<NormalizedEvent> {
    let attrs = &e.object_attributes;
    // The bot stamps every comment it posts with BOT_NOTE_MARKER. Drop those
    // here so the bot doesn't react to itself (matters especially when the
    // bot and operator share a GitLab account).
    if attrs.note.contains(BOT_NOTE_MARKER) {
        return None;
    }
    let target = match attrs.noteable_type.as_str() {
        "MergeRequest" => {
            let mr = e.merge_request.as_ref()?;
            NoteTargetRef::PullRequest {
                iid: mr.iid,
                source_branch: mr.source_branch.clone(),
                // GitLab note payload doesn't carry MR author/reviewers; the
                // dispatcher falls back to the actor check.
                author: None,
                reviewers: Vec::new(),
            }
        }
        "Issue" => {
            let issue = e.issue.as_ref()?;
            NoteTargetRef::Issue {
                iid: issue.iid,
                source_branch: None,
                // GitLab Issue payload only exposes assignee_ids (numeric); we
                // can't resolve usernames from the webhook alone.
                assignees: Vec::new(),
            }
        }
        _ => return None,
    };
    Some(NormalizedEvent {
        provider: ProviderKind::Gitlab,
        project: project_ref(&e.project),
        actor: e.user.username.clone(),
        kind: EventKind::NoteAdded {
            target,
            body: attrs.note.clone(),
            url: String::new(),
        },
    })
}
