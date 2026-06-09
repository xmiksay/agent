//! Dispatch a [`NormalizedEvent`] into the task store, or release a branch
//! when the underlying issue/PR closes.

use anyhow::Result;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::AppState;
use crate::jobs::types::TriggerReason;
use crate::project::NewProjectConfig;
use crate::service::{AuthKind, Service, TriggerMode};
use crate::webhook::normalized::{EventKind, NormalizedEvent, NoteTargetRef};
use crate::workspace::layout::slugify;

pub async fn dispatch(
    state: &AppState,
    service: &Service,
    ev: NormalizedEvent,
) -> Result<Vec<Uuid>> {
    let my_username = service.bot_username.clone();

    // Always upsert the project so we have a row + allowlist regardless of action.
    let (project, created) = state
        .project_store
        .upsert_project(NewProjectConfig {
            provider: ev.provider,
            service_id: service.id,
            project_slug: ev.project.project_slug.clone(),
            full_name: ev.project.full_name.clone(),
            remote_url: ev.project.remote_url.clone(),
            default_branch: ev.project.default_branch.clone(),
            my_username: my_username.clone(),
        })
        .await?;

    // First time we see a project, auto-register its webhook (idempotent on the
    // provider side). Best-effort and backgrounded so it never delays — or fails
    // — the inbound delivery; logged on error.
    if created {
        ensure_project_webhook(state, service, &project.full_name);
    }

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

    // A comment continues the issue/MR's existing agent: deliver it as a message
    // to that task (live to a warm agent, else resume its session) instead of
    // spawning a fresh, memory-less run on the shared branch. This reattach is
    // **independent of trigger mode/label/assignee/mention** — once a task exists
    // for the issue/MR, every follow-up comment reaches it. Only when no task
    // exists yet do the start-gates in `build_trigger`'s NoteAdded arm decide
    // whether a bare comment may spin up a fresh task.
    if let EventKind::NoteAdded { target, body, url } = &ev.kind
        && let Some(task_id) = resumable_task_for_note(state, project.id, target).await?
    {
        // Dedupe on the comment so a redelivered webhook doesn't double-post.
        let event_id = note_event_id(target, body);
        if state.task_store.is_duplicate(&event_id) {
            info!(event_id, "duplicate comment, skipping");
            return Ok(vec![]);
        }
        if !state.task_store.mark_seen(&event_id).await {
            info!(event_id, "duplicate comment in queue, skipping");
            return Ok(vec![]);
        }
        let _ = url;
        state.task_store.push_message(task_id, body.clone()).await?;
        info!(%task_id, "delivered comment to existing issue/MR agent");
        return Ok(vec![task_id]);
    }

    let trigger = match build_trigger(
        &ev,
        &my_username,
        service.trigger_mode,
        &service.trigger_label,
    ) {
        Some(t) => t,
        None => {
            info!(
                project = %ev.project.full_name,
                actor = %ev.actor,
                mode = %service.trigger_mode.as_str(),
                trigger_label = %service.trigger_label,
                "event matched no trigger — not for this agent (assignee/label/reviewer didn't match)"
            );
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

    let id = state.task_store.create_task(trigger, project.id).await?;
    info!(%id, project = %ev.project.full_name, autofire = service.autofire, "task created from webhook");

    // Autofire services skip the manual confirm step: start the task immediately.
    // A failed confirm must not fail dispatch — the task stays pending for a retry.
    if service.autofire
        && let Err(e) = state.task_store.confirm_task(id).await
    {
        warn!(%id, error = %e, "autofire confirm failed; leaving task pending");
    }

    Ok(vec![id])
}

/// Spawn idempotent webhook registration for a newly-seen project. No-op (logged)
/// when `PUBLIC_BASE_URL` is unset — operators then wire hooks by hand. Runs in
/// the background so it never delays or fails the inbound webhook delivery.
fn ensure_project_webhook(state: &AppState, service: &Service, full_name: &str) {
    // App-backed services receive events via a single app-level webhook (set on
    // the App itself), so per-repo registration is both unnecessary and outside
    // the App-token's scope.
    if service.auth_kind == AuthKind::App {
        debug!(project = %full_name, "app-auth service; using app-level webhook, skipping per-repo registration");
        return;
    }
    let Some(base) = state.config.public_base_url.clone() else {
        debug!(project = %full_name, "PUBLIC_BASE_URL unset; skipping webhook auto-registration");
        return;
    };
    let webhook_url = format!("{base}/webhook/{}/{}", service.kind.as_str(), service.slug);
    let secret = service.webhook_secret.clone();
    let service_id = service.id;
    let full_name = full_name.to_string();
    let providers = state.providers.clone();
    tokio::spawn(async move {
        let Some(provider) = providers.get(service_id).await else {
            warn!(%service_id, "provider not loaded; cannot auto-register webhook");
            return;
        };
        match provider
            .ensure_webhook(&full_name, &webhook_url, &secret)
            .await
        {
            Ok(()) => info!(project = %full_name, %webhook_url, "auto-registered webhook"),
            Err(e) => warn!(project = %full_name, error = %e, "webhook auto-registration failed"),
        }
    });
}

/// The existing resumable task an incoming comment should reattach to, found
/// straight from the note's target (MR source branch / issue iid → branch) —
/// **no trigger gate**. `None` when nothing is tracked for that issue/MR yet, so
/// the caller falls through to the normal start-gates.
async fn resumable_task_for_note(
    state: &AppState,
    project_id: Uuid,
    target: &NoteTargetRef,
) -> Result<Option<Uuid>> {
    let branch = match target {
        NoteTargetRef::PullRequest { source_branch, .. } => source_branch.clone(),
        NoteTargetRef::Issue { iid, .. } => {
            match state
                .project_store
                .find_branch_for_issue(project_id, *iid as i64)
                .await?
            {
                Some(b) => b.branch_name,
                None => return Ok(None),
            }
        }
    };
    state
        .task_store
        .find_resumable_task_for_branch(project_id, &branch)
        .await
}

/// Dedupe key for a comment reattach. Mirrors the `event_id()` a comment
/// `TriggerReason` would produce, so a redelivered webhook is deduped whether it
/// reattaches to an existing task or (first comment) starts a fresh one.
fn note_event_id(target: &NoteTargetRef, body: &str) -> String {
    let trigger = match target {
        NoteTargetRef::PullRequest {
            iid, source_branch, ..
        } => TriggerReason::MRComment {
            mr_iid: *iid,
            comment: body.to_string(),
            source_branch: source_branch.clone(),
            url: String::new(),
        },
        NoteTargetRef::Issue { iid, .. } => TriggerReason::IssueComment {
            issue_iid: *iid,
            comment: body.to_string(),
            url: String::new(),
        },
    };
    trigger.event_id()
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

fn build_trigger(
    ev: &NormalizedEvent,
    my_username: &str,
    trigger_mode: TriggerMode,
    trigger_label: &str,
) -> Option<TriggerReason> {
    match &ev.kind {
        EventKind::IssueAssigned {
            iid,
            assignees,
            labels,
            title,
            body,
            url,
        }
        | EventKind::IssueUpdated {
            iid,
            assignees,
            labels,
            title,
            body,
            url,
        } => {
            let kind = match &ev.kind {
                EventKind::IssueAssigned { .. } => "assigned",
                _ => "updated",
            };
            let by_assignee = matches!(trigger_mode, TriggerMode::Assignee | TriggerMode::Both)
                && assignees.iter().any(|a| a == my_username);
            let by_label = matches!(trigger_mode, TriggerMode::Label | TriggerMode::Both)
                && !trigger_label.is_empty()
                && labels.iter().any(|l| l == trigger_label);
            let matched = by_assignee || by_label;
            info!(
                iid,
                kind,
                bot = %my_username,
                mode = trigger_mode.as_str(),
                label = %trigger_label,
                assignees = ?assignees,
                labels = ?labels,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProviderKind;
    use crate::webhook::normalized::ProjectRef;

    fn issue_event(assignees: Vec<&str>, labels: Vec<&str>) -> NormalizedEvent {
        NormalizedEvent {
            provider: ProviderKind::Github,
            project: ProjectRef {
                full_name: "acme/repo".into(),
                project_slug: "acme__repo".into(),
                remote_url: "git@host:acme/repo.git".into(),
                default_branch: "main".into(),
            },
            actor: "someone".into(),
            kind: EventKind::IssueAssigned {
                iid: 7,
                assignees: assignees.into_iter().map(String::from).collect(),
                labels: labels.into_iter().map(String::from).collect(),
                title: "T".into(),
                body: "B".into(),
                url: "u".into(),
            },
        }
    }

    #[test]
    fn label_mode_matches_via_label_not_assignee() {
        let ev = issue_event(vec!["other"], vec!["agent"]);
        // Label mode ignores assignees entirely; the watched label fires it.
        assert!(build_trigger(&ev, "bot", TriggerMode::Label, "agent").is_some());
    }

    #[test]
    fn label_mode_ignores_assignee_match() {
        let ev = issue_event(vec!["bot"], vec!["something-else"]);
        // Bot is assigned but mode is label-only and the label doesn't match.
        assert!(build_trigger(&ev, "bot", TriggerMode::Label, "agent").is_none());
    }

    #[test]
    fn label_mode_with_empty_label_never_matches() {
        let ev = issue_event(vec!["bot"], vec!["agent"]);
        assert!(build_trigger(&ev, "bot", TriggerMode::Label, "").is_none());
    }

    #[test]
    fn assignee_mode_matches_via_assignee_not_label() {
        let ev = issue_event(vec!["bot"], vec!["agent"]);
        // Assignee mode ignores labels; assignment fires it.
        assert!(build_trigger(&ev, "bot", TriggerMode::Assignee, "agent").is_some());
    }

    #[test]
    fn assignee_mode_ignores_label_match() {
        let ev = issue_event(vec!["other"], vec!["agent"]);
        // The watched label is present but mode is assignee-only.
        assert!(build_trigger(&ev, "bot", TriggerMode::Assignee, "agent").is_none());
    }

    #[test]
    fn both_mode_matches_on_either() {
        let by_label = issue_event(vec!["other"], vec!["agent"]);
        let by_assignee = issue_event(vec!["bot"], vec!["x"]);
        let neither = issue_event(vec!["other"], vec!["x"]);
        assert!(build_trigger(&by_label, "bot", TriggerMode::Both, "agent").is_some());
        assert!(build_trigger(&by_assignee, "bot", TriggerMode::Both, "agent").is_some());
        assert!(build_trigger(&neither, "bot", TriggerMode::Both, "agent").is_none());
    }
}
