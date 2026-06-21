//! Dispatch a [`NormalizedEvent`] into the task store, or release a branch
//! when the underlying issue/PR closes.

use anyhow::Result;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::AppState;
use crate::jobs::create::review_branch_name;
use crate::jobs::types::TriggerReason;
use crate::project::NewProjectConfig;
use crate::service::{AuthKind, Service};
use crate::webhook::normalized::{EventKind, NormalizedEvent, NoteTargetRef};
use crate::webhook::trigger::build_trigger;
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
            // A ReviewMR ran on a separate `<source>-review` worktree; release it
            // too. A missing branch row is a logged no-op, so this is safe even
            // when no review ever ran for this PR.
            release_for_branch(
                state,
                project.id,
                &service.slug,
                &project.project_slug,
                &review_branch_name(source_branch),
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

    // Per-trigger-type gating. The reattach path above runs FIRST and is NOT
    // gated — a follow-up comment always reaches its existing task. Only the
    // fresh-start `build_trigger` arms below consult these configs. A load
    // failure must not fail dispatch: default to empty (all enabled, service
    // defaults).
    let trigger_cfgs = state
        .service_store
        .trigger_configs(service.id)
        .await
        .unwrap_or_default();

    let trigger = match build_trigger(
        &ev,
        &my_username,
        service.trigger_mode,
        &service.trigger_label,
        &trigger_cfgs,
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

    // Issue dedup (issue #35): one task per issue. If a task already tracks this
    // issue, refresh its stored description rather than spawning a duplicate —
    // editing the issue updates the existing work instead of forking it. (A
    // distinct edit clears the event_id dedupe above because that key hashes the
    // title+description; an identical re-fire is already dropped there.)
    if let TriggerReason::Issue {
        iid,
        title,
        description,
        ..
    } = &trigger
        && let Some(existing) = state.task_store.find_issue_task(project.id, *iid).await?
    {
        state
            .task_store
            .update_issue_description(existing.id, title, description)
            .await?;
        info!(task_id = %existing.id, iid, "issue already tracked; updated description instead of creating a task");
        return Ok(vec![existing.id]);
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
