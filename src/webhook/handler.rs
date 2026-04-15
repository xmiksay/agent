use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use tracing::{info, warn};

use crate::jobs::types::TriggerReason;
use crate::webhook::types::*;
use crate::AppState;

#[derive(Serialize)]
pub struct WebhookResponse {
    pub task_id: uuid::Uuid,
}

pub async fn handle_webhook(
    State(state): State<AppState>,
    Json(event): Json<GitLabEvent>,
) -> Result<(StatusCode, Json<WebhookResponse>), StatusCode> {
    let trigger = match determine_trigger(&event, &state.config.gitlab_username) {
        Some(t) => t,
        None => {
            info!("event received but no matching trigger, ignoring");
            return Err(StatusCode::OK);
        }
    };

    let event_id = trigger.event_id();
    if state.task_store.is_duplicate(&event_id) {
        info!(event_id, "duplicate event, skipping");
        return Err(StatusCode::OK);
    }

    if !state.task_store.mark_seen(&event_id).await {
        info!(event_id, "duplicate event in queue, skipping");
        return Err(StatusCode::OK);
    }

    info!(?trigger, "creating pending task");

    let project = match &event {
        GitLabEvent::Issue(e) => &e.project,
        GitLabEvent::MergeRequest(e) => &e.project,
        GitLabEvent::Note(e) => &e.project,
    };

    let task_id = state
        .task_store
        .create_task(
            trigger,
            project.path_with_namespace.clone(),
            project.git_ssh_url.clone(),
            project.default_branch.clone(),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((StatusCode::CREATED, Json(WebhookResponse { task_id })))
}

fn determine_trigger(event: &GitLabEvent, my_username: &str) -> Option<TriggerReason> {
    match event {
        GitLabEvent::Issue(e) => {
            let action = e.object_attributes.action.as_deref()?;
            if !matches!(action, "open" | "update") {
                return None;
            }
            let assigned = e.object_attributes.assignees
                .iter()
                .any(|a| a.username == my_username);
            if !assigned {
                warn!(action, "issue event but not assigned to me");
                return None;
            }
            Some(TriggerReason::Issue {
                iid: e.object_attributes.iid,
                title: e.object_attributes.title.clone(),
                description: e.object_attributes.description.clone().unwrap_or_default(),
                url: e.object_attributes.url.clone(),
            })
        }
        GitLabEvent::MergeRequest(e) => {
            let action = e.object_attributes.action.as_deref()?;
            match action {
                "open" | "update" => {
                    let reviewing = e.object_attributes.reviewers
                        .iter()
                        .any(|r| r.username == my_username);
                    if !reviewing {
                        return None;
                    }
                    Some(TriggerReason::ReviewMR {
                        iid: e.object_attributes.iid,
                        title: e.object_attributes.title.clone(),
                        source_branch: e.object_attributes.source_branch.clone(),
                        target_branch: e.object_attributes.target_branch.clone(),
                        url: e.object_attributes.url.clone(),
                    })
                }
                "unapproval" => {
                    Some(TriggerReason::FixReview {
                        iid: e.object_attributes.iid,
                        title: e.object_attributes.title.clone(),
                        source_branch: e.object_attributes.source_branch.clone(),
                        url: e.object_attributes.url.clone(),
                    })
                }
                _ => None,
            }
        }
        GitLabEvent::Note(e) => {
            if !e.object_attributes.note.contains("@claude") {
                return None;
            }
            if e.object_attributes.noteable_type == "MergeRequest" {
                let mr = e.merge_request.as_ref()?;
                Some(TriggerReason::MRComment {
                    mr_iid: mr.iid,
                    comment: e.object_attributes.note.clone(),
                    source_branch: mr.source_branch.clone(),
                    url: mr.url.clone(),
                })
            } else if e.object_attributes.noteable_type == "Issue" {
                let issue = e.issue.as_ref()?;
                Some(TriggerReason::IssueComment {
                    issue_iid: issue.iid,
                    comment: e.object_attributes.note.clone(),
                    url: issue.url.clone(),
                })
            } else {
                None
            }
        }
    }
}
