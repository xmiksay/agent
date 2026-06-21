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
use crate::provider::{GitProvider, resolve_token};
use crate::service::ServiceCredentials;

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
    token_env: &str,
    creds: &ServiceCredentials,
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
        match push_changes(work_dir, token_env, creds).await {
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
async fn push_changes(work_dir: &str, token_env: &str, creds: &ServiceCredentials) -> Result<bool> {
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
    // Re-resolve just-in-time: a GitHub App installation token expires after ~1h,
    // and over a long multi-turn session the start-of-session value goes stale.
    // `resolve_token` is cheap for a PAT (returns the stored token) and consults
    // the refreshing cache for an App, re-minting only near expiry (#44).
    let token = resolve_token(creds)
        .await
        .context("resolving provider token for push")?;
    // `-u origin HEAD` pushes the current branch to a same-named remote branch
    // and sets upstream — required for a freshly created issue branch that has
    // no upstream yet; idempotent for branches that already track a remote.
    // The token lives only in this child's environment; the repo's persisted
    // credential helper reads it (token-HTTPS transport, see workspace::git).
    let push = Command::new("git")
        .args(["push", "-u", "origin", "HEAD"])
        .current_dir(work_dir)
        .env(token_env, &token)
        .status()
        .await
        .context("failed to git push")?;

    if !push.success() {
        bail!("git push failed");
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProviderKind;
    use crate::provider::NoteTarget;
    use std::process::Command as StdCommand;
    use tokio::sync::Mutex as AsyncMutex;

    /// A `GitProvider` that records every posted note instead of hitting the wire,
    /// so the finalize note path can be exercised offline.
    struct RecordingProvider {
        notes: AsyncMutex<Vec<(NoteTarget, String)>>,
    }

    impl RecordingProvider {
        fn new() -> Self {
            Self {
                notes: AsyncMutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl GitProvider for RecordingProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::Github
        }
        fn service_id(&self) -> Uuid {
            Uuid::nil()
        }
        async fn post_note(&self, _project: &str, target: NoteTarget, body: &str) -> Result<()> {
            self.notes.lock().await.push((target, body.to_string()));
            Ok(())
        }
        async fn ensure_webhook(&self, _repo: &str, _url: &str, _secret: &str) -> Result<()> {
            Ok(())
        }
    }

    fn result(is_error: bool, msg: &str) -> ClaudeOutput {
        ClaudeOutput {
            result: msg.to_string(),
            session_id: "sid".to_string(),
            total_cost_usd: 0.1234,
            is_error,
            num_turns: 3,
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    /// A successful issue turn posts a completion note to the *issue* target with
    /// the cost/turn summary and no error text.
    #[tokio::test]
    async fn post_result_issue_completion_targets_issue() {
        let provider = RecordingProvider::new();
        let trigger = TriggerReason::Issue {
            iid: 42,
            title: "t".into(),
            description: String::new(),
            url: "u".into(),
        };
        post_result(&trigger, &result(false, "ok"), "acme/widgets", &provider)
            .await
            .expect("post note");

        let notes = provider.notes.lock().await;
        assert_eq!(notes.len(), 1);
        assert!(matches!(notes[0].0, NoteTarget::Issue(42)));
        assert!(notes[0].1.contains("(completed)"));
        assert!(notes[0].1.contains("Task completed"));
        assert!(notes[0].1.contains("$0.1234"));
        assert!(notes[0].1.contains("Turns: 3"));
    }

    /// An errored MR-comment turn posts an *error* note to the merge-request
    /// target, surfacing the agent's error text.
    #[tokio::test]
    async fn post_result_mr_error_targets_merge_request() {
        let provider = RecordingProvider::new();
        let trigger = TriggerReason::MRComment {
            mr_iid: 7,
            comment: "c".into(),
            source_branch: "feature".into(),
            url: "u".into(),
        };
        post_result(&trigger, &result(true, "boom"), "acme/widgets", &provider)
            .await
            .expect("post note");

        let notes = provider.notes.lock().await;
        assert_eq!(notes.len(), 1);
        assert!(matches!(notes[0].0, NoteTarget::MergeRequest(7)));
        assert!(notes[0].1.contains("(error)"));
        assert!(notes[0].1.contains("Error: boom"));
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = StdCommand::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .expect("spawn git")
            .success();
        assert!(ok, "git {args:?} failed");
    }

    /// The push path must obtain its token by *calling* `resolve_token` per push
    /// (not from a captured constant), so a refreshed App token is picked up
    /// mid-session (#44). We exercise the seam end-to-end against a local bare
    /// remote: a `file://` remote ignores `GH_TOKEN`, so a real push succeeds
    /// while still routing through the `resolve_token(Pat)` arm.
    #[tokio::test]
    async fn push_changes_resolves_token_then_pushes_only_when_there_is_something_new() {
        let base = std::env::temp_dir().join(format!("agent-push-{}", uuid::Uuid::new_v4()));
        let remote = base.join("remote.git");
        let work = base.join("work");
        std::fs::create_dir_all(&remote).expect("mkdir remote");
        std::fs::create_dir_all(&work).expect("mkdir work");

        git(&remote, &["init", "--bare", "-q"]);
        git(&work, &["init", "-q"]);
        git(&work, &["config", "user.email", "t@t.t"]);
        git(&work, &["config", "user.name", "t"]);
        git(
            &work,
            &[
                "remote",
                "add",
                "origin",
                &format!("file://{}", remote.display()),
            ],
        );
        std::fs::write(work.join("f.txt"), "hello").expect("write file");
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", "init"]);
        // Establish the upstream the way the first successful turn would — the
        // #44 hot path is a *later* turn whose new commit is ahead of an
        // already-tracked branch.
        git(&work, &["push", "-q", "-u", "origin", "HEAD"]);

        let work_str = work.to_string_lossy().into_owned();
        let creds = ServiceCredentials::Pat("ignored-by-file-remote".into());

        // A later turn made a new commit, ahead of the upstream → a push happens,
        // routed through `resolve_token(creds)` (not a captured constant).
        std::fs::write(work.join("g.txt"), "more").expect("write file");
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", "turn 2"]);
        let pushed = push_changes(&work_str, "GH_TOKEN", &creds)
            .await
            .expect("ahead push_changes");
        assert!(pushed, "expected the new commit to be pushed");

        // Next call: nothing new (clean tree, upstream up to date) → no push.
        let pushed_again = push_changes(&work_str, "GH_TOKEN", &creds)
            .await
            .expect("idempotent push_changes");
        assert!(!pushed_again, "expected no push when nothing changed");

        let _ = std::fs::remove_dir_all(&base);
    }
}
