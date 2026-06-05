use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{info, warn, error};


use crate::agent::{AgentBackend, ClaudeCode};
use crate::config::Config;
use crate::git_service::GitService;
use crate::jobs::hub::LiveSessions;
use crate::jobs::output_log::TaskOutputLog;
use crate::jobs::prompt::build_prompt;
use crate::jobs::stream::{Stream, stream_into_entry, tail};
use crate::jobs::types::{ClaudeOutput, TriggerReason};
use crate::project::{BranchStatus, NewBranchEntry, ProjectStore, ProviderKind};
use crate::provider::GitProvider;
use crate::workspace::Workspace;
use crate::workspace::layout::slugify;

pub async fn run_job(
    task_id: uuid::Uuid,
    trigger: TriggerReason,
    service: GitService,
    project_id: Option<uuid::Uuid>,
    git_url: String,
    project_path: String,
    default_branch: String,
    branch_override: Option<String>,
    config: Config,
    provider: Arc<dyn GitProvider>,
    workspace: Arc<Workspace>,
    project_store: Arc<ProjectStore>,
    output_log: TaskOutputLog,
    hub: LiveSessions,
    resume_session_id: Option<String>,
    prompt_override: Option<String>,
    mut pid_tx: Option<tokio::sync::oneshot::Sender<u32>>,
    session_tx: Option<tokio::sync::oneshot::Sender<String>>,
) -> Result<ClaudeOutput> {
    let project_slug = slugify(&project_path);
    // The branch is derived and persisted at task-creation time (TaskStore::
    // create_task), so it's always present here. Guard against ever operating
    // on the project default branch — no trigger type legitimately targets it.
    let branch = branch_override.ok_or_else(|| anyhow::anyhow!("task has no branch"))?;
    if branch == default_branch {
        bail!("refusing to run task on default branch '{default_branch}'");
    }
    let branch_slug = slugify(&branch);

    let work_dir = workspace.branch_dir(&service.slug, &project_slug, &branch_slug);

    info!(
        service = %service.slug,
        project = %project_path,
        branch = %branch,
        path = %work_dir.display(),
        "ensuring branch checkout"
    );

    let _guard = workspace
        .lock_branch(&service.slug, &project_slug, &branch_slug)
        .await
        .context("locking branch workspace")?;

    // git_url is the SSH URL (git@host:path.git) populated by the webhook
    // normalizers. The agent host has SSH keys configured for the bot user, so
    // we clone/fetch directly — no token injection.
    workspace.clone_or_fetch(&work_dir, &git_url, &branch, &default_branch).await?;

    // Hardcoded to Claude Code for now; a future per-task config will choose.
    let backend: Arc<dyn AgentBackend> = Arc::new(ClaudeCode);
    write_worktree_files(&work_dir, backend.worktree_files(&workspace.authcheck_hook_path()))
        .await
        .context("writing agent worktree files")?;

    if let Some(pid) = project_id {
        let issue_iid = trigger.issue_iid().map(|v| v as i64);
        let pr_iid = trigger.pr_iid().map(|v| v as i64);
        project_store
            .upsert_branch(
                pid,
                NewBranchEntry {
                    branch_name: branch.clone(),
                    branch_slug: branch_slug.clone(),
                    issue_iid,
                    pr_iid,
                    status: BranchStatus::Active,
                },
            )
            .await
            .context("upserting branch state")?;
    }

    drop(_guard);

    let prompt = match prompt_override {
        Some(p) if !p.trim().is_empty() => p,
        _ => build_prompt(&trigger, &branch),
    };
    info!(%prompt, program = backend.program(), "running agent");

    let agent_port = config
        .listen_addr
        .rsplit_once(':')
        .map(|(_, p)| p.to_string())
        .unwrap_or_else(|| "3000".to_string());
    let agent_args = backend.build_args(resume_session_id.as_deref());
    let command_line = format!("{} {}", backend.program(), agent_args.join(" "));

    let entry = if resume_session_id.is_some() {
        output_log.resume_or_start(task_id, command_line.clone()).await
    } else {
        output_log.start(task_id, command_line.clone()).await
    };

    // So `gh`/`glab` inside the worktree authenticate against the same PAT the
    // agent uses to clone and post notes — picked by provider kind.
    let (provider_token_var, provider_token_value) = match service.kind {
        ProviderKind::Github => ("GH_TOKEN", service.token.clone()),
        ProviderKind::Gitlab => ("GITLAB_TOKEN", service.token.clone()),
    };

    let mut cmd = Command::new(backend.program());
    cmd.args(&agent_args).current_dir(&work_dir);
    for (key, value) in backend.extra_env(task_id, &agent_port) {
        cmd.env(key, value);
    }
    let mut child = cmd
        .env(provider_token_var, provider_token_value)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("failed to spawn agent")?;

    if let Some(pid) = child.id() {
        info!(%task_id, pid, "agent process running");
        if let Some(tx) = pid_tx.take() {
            let _ = tx.send(pid);
        }
    }

    // Interactive stdin: a pump task drains operator messages from the hub into
    // the child's stdin. The initial prompt is the first message. Holding the
    // ChildStdin keeps the session alive; the hub's `stop`/`end` drops the
    // sender, the pump sees the channel close, drops ChildStdin → EOF → claude
    // finishes the current turn and exits.
    let mut child_stdin = child.stdin.take().expect("piped stdin");
    let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<String>(32);
    let stdin_pump = tokio::spawn(async move {
        while let Some(line) = stdin_rx.recv().await {
            if child_stdin.write_all(line.as_bytes()).await.is_err()
                || child_stdin.write_all(b"\n").await.is_err()
                || child_stdin.flush().await.is_err()
            {
                break;
            }
        }
    });
    // Send the initial prompt, then hand the sender to the hub (only the hub
    // holds a sender now, so a graceful Stop closes stdin deterministically).
    stdin_tx
        .send(backend.encode_user_message(&prompt))
        .await
        .context("failed to queue initial prompt")?;
    hub.register(task_id, backend.clone(), stdin_tx).await;

    let stdout_pipe = child.stdout.take().expect("piped stdout");
    let stderr_pipe = child.stderr.take().expect("piped stderr");

    // Token-budget abort: stream reader counts output_tokens from each event's
    // `usage` field and fires this oneshot once cumulative use hits 50% of the
    // configured budget. Aborting via SIGKILL is fine — session_id was already
    // captured from the init event, so the task can Resume after rate limit
    // resets.
    let (budget_tx, budget_rx) = tokio::sync::oneshot::channel::<u64>();
    let token_limit = config.task_token_budget / 2;

    let stdout_reader = tokio::spawn(stream_into_entry(
        stdout_pipe,
        entry.clone(),
        Stream::Stdout,
        backend.clone(),
        hub.clone(),
        task_id,
        session_tx,
        Some((token_limit, budget_tx)),
    ));
    let stderr_reader = tokio::spawn(stream_into_entry(
        stderr_pipe,
        entry.clone(),
        Stream::Stderr,
        backend.clone(),
        hub.clone(),
        task_id,
        None,
        None,
    ));

    // No timeout — claude tasks can run as long as they need. Operator can
    // Pause/Kill from the UI if a task is stuck. We do enforce a token budget
    // (above) which races against child.wait().
    let exit_status = tokio::select! {
        res = child.wait() => res.context("waiting for claude exit")?,
        used = budget_rx => {
            let used = used.unwrap_or_default();
            warn!(%task_id, used, limit = token_limit, "token budget reached, killing claude");
            let _ = child.start_kill();
            let status = child.wait().await.context("waiting for claude exit after kill")?;
            // Surface as a graceful pause: the outer save path treats non-zero
            // exits as failure, but session_id is set so Resume works.
            status
        }
    };
    let exit_code = exit_status.code();

    // The child is gone; stop the stdin pump and drain the readers (they finish
    // once the pipes hit EOF).
    stdin_pump.abort();
    let _ = stdout_reader.await;
    let _ = stderr_reader.await;

    // Close the live session: flush the unpersisted event tail to event_log and
    // drop the channel so any attached WebSocket finishes.
    hub.end(task_id).await;

    // Snapshot the streamed state for downstream use.
    let (stdout, stderr) = {
        let mut guard = entry.lock().await;
        guard.exit_code = exit_code;
        guard.finished = true;
        (guard.stdout.clone(), guard.stderr.clone())
    };

    info!(
        exit_code = ?exit_code,
        stdout_bytes = stdout.len(),
        stderr_bytes = stderr.len(),
        "claude process exited"
    );

    if !exit_status.success() {
        // The full stdout/stderr are already in the in-memory output log and
        // rendered nicely by the Command output panel. Keep this error short
        // (it ends up in task_results.result_text) — only include the stderr
        // tail because that's where curl/launch failures show up.
        let stderr_tail = tail(stderr.trim(), 600);
        if stderr_tail.is_empty() {
            bail!("claude exited with status {:?}", exit_code);
        } else {
            bail!(
                "claude exited with status {:?}\nstderr tail:\n{stderr_tail}",
                exit_code
            );
        }
    }

    let claude_result = backend.parse_result(&stdout).with_context(|| {
        let stderr_tail = tail(stderr.trim(), 600);
        if stderr_tail.is_empty() {
            "no result event in agent output".to_string()
        } else {
            format!("no result event in agent output\nstderr tail:\n{stderr_tail}")
        }
    })?;

    info!(
        cost = claude_result.total_cost_usd,
        turns = claude_result.num_turns,
        is_error = claude_result.is_error,
        "claude finished"
    );

    if claude_result.is_error {
        error!(result = %claude_result.result, "claude returned error");
    }

    // Post result back to provider
    post_result(&trigger, &claude_result, &project_path, provider.as_ref()).await?;

    // Push any changes for workflows that produce code edits.
    if matches!(
        trigger,
        TriggerReason::Issue { .. }
            | TriggerReason::FixReview { .. }
            | TriggerReason::MRComment { .. }
            | TriggerReason::IssueComment { .. }
    ) {
        push_changes(work_dir.to_string_lossy().as_ref()).await?;
    }

    // NOTE: checkout is intentionally NOT removed here — see workspace lifecycle.
    Ok(claude_result)
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

    let status = if result.is_error { "error" } else { "completed" };
    let body = format!(
        "**Claude Agent** ({status})\n\n\
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

async fn push_changes(work_dir: &str) -> Result<()> {
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
                return Ok(());
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

    Ok(())
}

/// Materialize the backend's per-run worktree files (agent config, hooks),
/// creating parent directories as needed.
async fn write_worktree_files(
    work_dir: &std::path::Path,
    files: Vec<crate::agent::WorktreeFile>,
) -> anyhow::Result<()> {
    for file in files {
        let path = work_dir.join(&file.rel_path);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        tokio::fs::write(&path, file.contents)
            .await
            .with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

