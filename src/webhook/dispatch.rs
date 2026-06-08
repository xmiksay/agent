//! Dispatch a [`NormalizedEvent`] into the task store, or release a branch
//! when the underlying issue/PR closes.

use anyhow::Result;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::AppState;
use crate::git_service::GitService;
use crate::jobs::types::TriggerReason;
use crate::project::NewProjectConfig;
use crate::webhook::normalized::{EventKind, NormalizedEvent, NoteTargetRef};
use crate::workspace::layout::slugify;

pub async fn dispatch(
    state: &AppState,
    service: &GitService,
    ev: NormalizedEvent,
) -> Result<Vec<Uuid>> {
    let my_username = service.bot_username.clone();

    // Always upsert the project so we have a row + allowlist regardless of action.
    let project = state
        .project_store
        .upsert_project(NewProjectConfig {
            provider: ev.provider,
            git_service_id: service.id,
            project_slug: ev.project.project_slug.clone(),
            full_name: ev.project.full_name.clone(),
            ssh_url: ev.project.ssh_url.clone(),
            default_branch: ev.project.default_branch.clone(),
            my_username: my_username.clone(),
        })
        .await?;

    // Lifecycle: close events release the branch and exit early.
    match &ev.kind {
        EventKind::IssueClosed { iid, .. } => {
            release_for_issue(
                state,
                project.id,
                &service.slug,
                &project.project_slug,
                *iid as i64,
            )
            .await?;
            return Ok(vec![]);
        }
        EventKind::PrClosed { source_branch, .. } => {
            release_for_branch(
                state,
                project.id,
                &service.slug,
                &project.project_slug,
                source_branch,
            )
            .await?;
            return Ok(vec![]);
        }
        _ => {}
    }

    let trigger = match build_trigger(&ev, &my_username) {
        Some(t) => t,
        None => {
            debug!(actor = %ev.actor, "no trigger for normalized event");
            return Ok(vec![]);
        }
    };

    let event_id = trigger.event_id();
    if state.task_store.is_duplicate(&event_id) {
        info!(event_id, "duplicate event, skipping");
        return Ok(vec![]);
    }
    if !state.task_store.mark_seen(&event_id).await {
        info!(event_id, "duplicate event in queue, skipping");
        return Ok(vec![]);
    }

    // A comment continues the issue/MR's existing agent: deliver it as a message
    // to that task (live to a warm agent, else resume its session) instead of
    // spawning a fresh, memory-less run on the shared branch. Falls back to a new
    // task when there's no resumable one yet (e.g. the first interaction is a
    // comment).
    if let Some(task_id) = resumable_task_for_comment(state, project.id, &trigger).await? {
        let comment = match &trigger {
            TriggerReason::IssueComment { comment, .. }
            | TriggerReason::MRComment { comment, .. } => comment.clone(),
            _ => unreachable!("resumable_task_for_comment only matches comment triggers"),
        };
        state.task_store.push_message(task_id, comment).await?;
        info!(%task_id, "delivered comment to existing issue/MR agent");
        return Ok(vec![task_id]);
    }

    let id = state
        .task_store
        .create_task(
            trigger,
            service.id,
            ev.provider,
            Some(project.id),
            ev.project.full_name.clone(),
            ev.project.ssh_url.clone(),
            ev.project.default_branch.clone(),
        )
        .await?;

    // Autofire services skip the manual confirm step: start the task immediately.
    // A failed confirm must not fail dispatch — the task stays pending for a retry.
    if service.autofire
        && let Err(e) = state.task_store.confirm_task(id).await
    {
        warn!(%id, error = %e, "autofire confirm failed; leaving task pending");
    }

    Ok(vec![id])
}

/// For a comment trigger, the existing task on the issue/MR branch that can take
/// the comment as a follow-up (has a `session_id`). `None` for non-comment
/// triggers or when no such task exists yet.
async fn resumable_task_for_comment(
    state: &AppState,
    project_id: Uuid,
    trigger: &TriggerReason,
) -> Result<Option<Uuid>> {
    let branch = match trigger {
        TriggerReason::MRComment { source_branch, .. } => source_branch.clone(),
        TriggerReason::IssueComment { issue_iid, .. } => {
            match state
                .project_store
                .find_branch_for_issue(project_id, *issue_iid as i64)
                .await?
            {
                Some(b) => b.branch_name,
                None => return Ok(None),
            }
        }
        _ => return Ok(None),
    };
    state
        .task_store
        .find_resumable_task_for_branch(project_id, &branch)
        .await
}

async fn release_for_issue(
    state: &AppState,
    project_id: Uuid,
    service_slug: &str,
    project_slug: &str,
    issue_iid: i64,
) -> Result<()> {
    let Some(branch) = state
        .project_store
        .find_branch_for_issue(project_id, issue_iid)
        .await?
    else {
        debug!(
            issue_iid,
            "no checked-out branch bound to this issue, nothing to release"
        );
        return Ok(());
    };
    // Stop any live agent on this branch before reclaiming the worktree.
    stop_branch_agent(state, project_id, &branch.branch_name).await;
    let _g = state
        .workspace
        .lock_branch(service_slug, project_slug, &branch.branch_slug)
        .await?;
    state
        .workspace
        .remove_branch_dir(service_slug, project_slug, &branch.branch_slug)
        .await?;
    state
        .project_store
        .delete_branch(project_id, &branch.branch_slug)
        .await?;
    info!(branch = %branch.branch_name, "released branch on issue close");
    Ok(())
}

async fn release_for_branch(
    state: &AppState,
    project_id: Uuid,
    service_slug: &str,
    project_slug: &str,
    branch_name: &str,
) -> Result<()> {
    let branch_slug = slugify(branch_name);
    let Some(_existing) = state
        .project_store
        .find_branch(project_id, &branch_slug)
        .await?
    else {
        debug!(
            branch = branch_name,
            "branch not tracked, nothing to release"
        );
        return Ok(());
    };
    // Stop any live agent on this branch before reclaiming the worktree.
    stop_branch_agent(state, project_id, branch_name).await;
    let _g = state
        .workspace
        .lock_branch(service_slug, project_slug, &branch_slug)
        .await?;
    if let Err(e) = state
        .workspace
        .remove_branch_dir(service_slug, project_slug, &branch_slug)
        .await
    {
        warn!(error = %e, branch = branch_name, "failed to remove branch dir");
    }
    state
        .project_store
        .delete_branch(project_id, &branch_slug)
        .await?;
    info!(branch = branch_name, "released branch on PR close");
    Ok(())
}

/// Kill a live (running or warm-idle) agent bound to a branch so its worktree
/// can be reclaimed. Best-effort: a missing/already-finished task is a no-op.
async fn stop_branch_agent(state: &AppState, project_id: Uuid, branch_name: &str) {
    if let Ok(Some(task_id)) = state
        .task_store
        .find_resumable_task_for_branch(project_id, branch_name)
        .await
    {
        if let Err(e) = state.task_store.kill_task(task_id).await {
            debug!(%task_id, error = %e, "no live agent to stop on close");
        } else {
            info!(%task_id, branch = branch_name, "stopped agent on issue/MR close");
        }
    }
}

fn build_trigger(ev: &NormalizedEvent, my_username: &str) -> Option<TriggerReason> {
    match &ev.kind {
        EventKind::IssueAssigned {
            iid,
            assignees,
            title,
            body,
            url,
        }
        | EventKind::IssueUpdated {
            iid,
            assignees,
            title,
            body,
            url,
        } => {
            let kind = match &ev.kind {
                EventKind::IssueAssigned { .. } => "assigned",
                _ => "updated",
            };
            let matched = assignees.iter().any(|a| a == my_username);
            info!(
                iid,
                kind,
                bot = %my_username,
                assignees = ?assignees,
                matched,
                title = %title,
                "issue event"
            );
            if !matched {
                return None;
            }
            Some(TriggerReason::Issue {
                iid: *iid,
                title: title.clone(),
                description: body.clone(),
                url: url.clone(),
            })
        }
        EventKind::IssueClosed { .. } => None, // lifecycle in Phase 7
        EventKind::PrReviewSubmitted {
            iid,
            source_branch,
            state,
            review_body,
            url,
            author,
            ..
        } => {
            use crate::webhook::normalized::ReviewState;
            // FixReview is the "fix comments on my own MR" workflow, so the gate
            // is "bot authored the MR". GitHub gives us the author directly.
            // GitLab webhooks only expose `author_id` (numeric); the bot doesn't
            // submit reviews via post_note, so there's no echo to guard against
            // and we trust an unknown author as the bot's MR.
            let is_my_mr = match author {
                Some(a) => a == my_username,
                None => true,
            };
            if !is_my_mr {
                info!(iid, bot = %my_username, author = ?author, actor = %ev.actor, "review event on PR not authored by bot, skipping");
                return None;
            }
            // A bare approval with no body has nothing to address.
            if matches!(state, ReviewState::Approved) && review_body.trim().is_empty() {
                return None;
            }
            Some(TriggerReason::FixReview {
                iid: *iid,
                title: String::new(),
                source_branch: source_branch.clone(),
                url: url.clone(),
                review_body: review_body.clone(),
            })
        }
        EventKind::ReviewRequested {
            iid,
            source_branch,
            target_branch,
            url,
            reviewers,
            title,
        } => {
            if !reviewers.iter().any(|r| r == my_username) {
                return None;
            }
            Some(TriggerReason::ReviewMR {
                iid: *iid,
                title: title.clone(),
                source_branch: source_branch.clone(),
                target_branch: target_branch.clone(),
                url: url.clone(),
            })
        }
        EventKind::PrClosed { .. } => None, // lifecycle in Phase 7
        EventKind::NoteAdded { target, body, url } => {
            let mention = format!("@{my_username}");
            let mentioned = body.contains(&mention);
            match target {
                NoteTargetRef::PullRequest {
                    iid,
                    source_branch,
                    author,
                    reviewers,
                } => {
                    // Mentions always count. Otherwise only act if the bot owns
                    // (authored) the MR or is a reviewer on it. The bot's own
                    // posts are stamped with BOT_NOTE_MARKER and filtered at
                    // source in normalize_note, so we don't need an actor-based
                    // loop guard here — and we trust an unknown author as the
                    // bot's MR (matters for shared-account GitLab setups).
                    let is_my_mr = match author {
                        Some(a) => a == my_username,
                        None => true,
                    };
                    let is_reviewer = reviewers.iter().any(|r| r == my_username);
                    if !mentioned && !is_my_mr && !is_reviewer {
                        return None;
                    }
                    Some(TriggerReason::MRComment {
                        mr_iid: *iid,
                        comment: body.clone(),
                        source_branch: source_branch.clone(),
                        url: url.clone(),
                    })
                }
                NoteTargetRef::Issue { iid, assignees, .. } => {
                    // Bot reacts to comments on issues it is assigned to.
                    // GitLab can't expose assignee usernames in the note payload;
                    // there we still require an @-mention of the bot.
                    let is_my_issue = assignees.iter().any(|a| a == my_username);
                    if !mentioned && !is_my_issue {
                        return None;
                    }
                    Some(TriggerReason::IssueComment {
                        issue_iid: *iid,
                        comment: body.clone(),
                        url: url.clone(),
                    })
                }
            }
        }
    }
}
