//! Per-turn finalization for an interactive agent session: persist the turn's
//! result, push commits, and post a reply note "on demand".

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::agent::AgentBackend;
use crate::jobs::store::TaskStore;
use crate::jobs::types::{ClaudeOutput, TriggerReason};
use crate::provider::GitProvider;

/// After a turn: snapshot the agent's last `result`, persist it, push any
/// commits (for code-producing triggers), and post a reply note only when there
/// is something worth reporting — commits landed or the turn errored. All
/// best-effort: a turn's bookkeeping failure must not tear down the session.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finalize_turn(
    result_event: serde_json::Value,
    backend: &dyn AgentBackend,
    store: &Arc<TaskStore>,
    trigger: &TriggerReason,
    project_path: &str,
    provider: &dyn GitProvider,
    work_dir: &str,
    task_id: Uuid,
    code_trigger: bool,
) {
    let result = match backend.parse_result(&result_event.to_string()) {
        Ok(r) => r,
        Err(e) => {
            warn!(%task_id, error = %e, "turn produced no parseable result event");
            return;
        }
    };
    info!(
        %task_id,
        cost = result.total_cost_usd,
        turns = result.num_turns,
        is_error = result.is_error,
        "turn finished"
    );
    if let Err(e) = store.replace_result(task_id, &result).await {
        error!(%task_id, error = %e, "failed to save turn result");
    }

    let pushed = if code_trigger {
        match push_changes(work_dir).await {
            Ok(p) => p,
            Err(e) => {
                error!(%task_id, error = %e, "failed to push turn changes");
                false
            }
        }
    } else {
        false
    };

    // Reply on demand: only when commits landed or the turn errored.
    if (pushed || result.is_error)
        && let Err(e) = post_result(trigger, &result, project_path, provider).await
    {
        error!(%task_id, error = %e, "failed to post turn note");
    }
}

async fn post_result(
    trigger: &TriggerReason,
    result: &ClaudeOutput,
    project_path: &str,
    provider: &dyn GitProvider,
) -> Result<()> {
    use crate::provider::NoteTarget;
    let target = match trigger {
        TriggerReason::Issue { iid, .. } | TriggerReason::IssueComment { issue_iid: iid, .. } => {
            NoteTarget::Issue(*iid)
        }
        TriggerReason::ReviewMR { iid, .. }
        | TriggerReason::FixReview { iid, .. }
        | TriggerReason::MRComment { mr_iid: iid, .. } => NoteTarget::MergeRequest(*iid),
    };

    let status = if result.is_error {
        "error"
    } else {
        "completed"
    };
    let body = format!(
        "**Agent** ({status})\n\n\
         Cost: ${:.4} | Turns: {}\n\n\
         {}",
        result.total_cost_usd,
        result.num_turns,
        if result.is_error {
            format!("Error: {}", result.result)
        } else {
            "Task completed. See commits for changes.".to_string()
        }
    );

    provider.post_note(project_path, target, &body).await
}

/// Push the branch if there's anything new. Returns whether a push actually
/// happened (used to decide whether a turn is worth a reply note).
async fn push_changes(work_dir: &str) -> Result<bool> {
    let has_changes = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(work_dir)
        .output()
        .await?;

    if has_changes.stdout.is_empty() {
        let unpushed = Command::new("git")
            .args(["log", "@{u}..HEAD", "--oneline"])
            .current_dir(work_dir)
            .output()
            .await;

        match unpushed {
            Ok(out) if out.stdout.is_empty() => {
                info!("no changes to push");
                return Ok(false);
            }
            _ => {}
        }
    }

    info!("pushing changes");
    // `-u origin HEAD` pushes the current branch to a same-named remote branch
    // and sets upstream — required for a freshly created issue branch that has
    // no upstream yet; idempotent for branches that already track a remote.
    let push = Command::new("git")
        .args(["push", "-u", "origin", "HEAD"])
        .current_dir(work_dir)
        .status()
        .await
        .context("failed to git push")?;

    if !push.success() {
        bail!("git push failed");
    }

    Ok(true)
}
