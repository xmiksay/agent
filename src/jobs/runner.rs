use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{info, warn, error};


use crate::config::Config;
use crate::git_service::GitService;
use crate::jobs::output_log::{LiveEntry, TaskOutputLog};
use crate::jobs::types::{ClaudeOutput, TriggerReason};
use crate::project::{BranchStatus, NewBranchEntry, ProjectStore};
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
    resume_session_id: Option<String>,
    mut pid_tx: Option<tokio::sync::oneshot::Sender<u32>>,
    session_tx: Option<tokio::sync::oneshot::Sender<String>>,
) -> Result<ClaudeOutput> {
    let project_slug = slugify(&project_path);
    let branch = branch_override
        .as_deref()
        .or_else(|| trigger.branch())
        .unwrap_or(&default_branch)
        .to_string();
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
        .lock_project(&service.slug, &project_slug)
        .await
        .context("locking project workspace")?;

    // git_url is the SSH URL (git@host:path.git) populated by the webhook
    // normalizers. The agent host has SSH keys configured for the bot user, so
    // we clone/fetch directly — no token injection.
    workspace.clone_or_fetch(&work_dir, &git_url, &branch).await?;
    write_settings_local(&work_dir, &workspace.authcheck_hook_path())
        .await
        .context("writing .claude/settings.local.json")?;

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

    let prompt = build_prompt(&trigger);
    info!(%prompt, "running claude");

    let agent_port = config
        .listen_addr
        .rsplit_once(':')
        .map(|(_, p)| p.to_string())
        .unwrap_or_else(|| "3000".to_string());
    // stream-json + --verbose emits newline-delimited JSON events as claude
    // works, so we can show progress while it runs. The last event is
    // `{"type":"result", ...}` with the same fields the old `json` format
    // returned in a single blob.
    let mut claude_args: Vec<String> = vec!["-p".into(), prompt.clone()];
    if let Some(sid) = resume_session_id.as_deref() {
        claude_args.push("-r".into());
        claude_args.push(sid.into());
    }
    claude_args.extend([
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ]);
    let command_line = format!("claude {}", claude_args.join(" "));

    let entry = if resume_session_id.is_some() {
        output_log.resume_or_start(task_id, command_line.clone()).await
    } else {
        output_log.start(task_id, command_line.clone()).await
    };

    let mut child = Command::new("claude")
        .args(&claude_args)
        .current_dir(&work_dir)
        .env("CLAUDE_TASK_ID", task_id.to_string())
        .env("AGENT_PORT", agent_port)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("failed to spawn claude")?;

    if let Some(pid) = child.id() {
        info!(%task_id, pid, "claude process running");
        if let Some(tx) = pid_tx.take() {
            let _ = tx.send(pid);
        }
    }

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
        session_tx,
        Some((token_limit, budget_tx)),
    ));
    let stderr_reader = tokio::spawn(stream_into_entry(
        stderr_pipe,
        entry.clone(),
        Stream::Stderr,
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

    // Drain readers (they'll finish once the pipes hit EOF).
    let _ = stdout_reader.await;
    let _ = stderr_reader.await;

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

    let claude_result = parse_stream_json_result(&stdout).with_context(|| {
        let stderr_tail = tail(stderr.trim(), 600);
        if stderr_tail.is_empty() {
            "no result event in claude stream-json output".to_string()
        } else {
            format!("no result event in claude stream-json output\nstderr tail:\n{stderr_tail}")
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

    // Push any changes for issue/fix workflows
    if matches!(
        trigger,
        TriggerReason::Issue { .. } | TriggerReason::FixReview { .. }
    ) {
        push_changes(work_dir.to_string_lossy().as_ref()).await?;
    }

    // NOTE: checkout is intentionally NOT removed here — see workspace lifecycle.
    Ok(claude_result)
}

fn build_prompt(trigger: &TriggerReason) -> String {
    match trigger {
        TriggerReason::Issue { iid, title, description, url, .. } => {
            format!(
                "Implement GitLab issue #{iid}: {title}\n\n\
                 Description:\n{description}\n\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Implement the issue\n\
                 - Create a new branch and commit your changes\n\
                 - Create a merge request using `glab mr create`"
            )
        }
        TriggerReason::ReviewMR { iid, title, source_branch, target_branch, url, .. } => {
            format!(
                "Review merge request !{iid}: {title}\n\
                 Branch: {source_branch} -> {target_branch}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Review the diff: `git diff {target_branch}...{source_branch}`\n\
                 - Post your review as a comment using `glab mr note {iid}`\n\
                 - If changes are needed, list them clearly\n\
                 - If everything looks good, approve with `glab mr approve {iid}`"
            )
        }
        TriggerReason::FixReview { iid, title, source_branch, url, .. } => {
            format!(
                "Fix review comments on MR !{iid}: {title}\n\
                 Branch: {source_branch}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Check review comments: `glab mr view {iid} --comments`\n\
                 - Address each comment\n\
                 - Commit and push fixes"
            )
        }
        TriggerReason::MRComment { mr_iid, comment, url, .. } => {
            format!(
                "Respond to comment on MR !{mr_iid}\n\
                 Comment: {comment}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Address the request in the comment\n\
                 - Reply using `glab mr note {mr_iid}`"
            )
        }
        TriggerReason::IssueComment { issue_iid, comment, url, .. } => {
            format!(
                "Respond to comment on issue #{issue_iid}\n\
                 Comment: {comment}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Address the request in the comment\n\
                 - Reply using `glab issue note {issue_iid}`"
            )
        }
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
    let push = Command::new("git")
        .args(["push"])
        .current_dir(work_dir)
        .status()
        .await
        .context("failed to git push")?;

    if !push.success() {
        bail!("git push failed");
    }

    Ok(())
}

/// Writes a per-task `.claude/settings.local.json` pointing Claude Code at the
/// shared authcheck hook. `settings.local.json` is the conventional Claude
/// Code per-machine override file (typically gitignored), so it does not need
/// to be committed by the project — projects that want it gitignored just need
/// `.claude/settings.local.json` in their `.gitignore` (Claude Code adds this
/// automatically on first run in most setups).
async fn write_settings_local(
    work_dir: &std::path::Path,
    hook_path: &std::path::Path,
) -> anyhow::Result<()> {
    let claude_dir = work_dir.join(".claude");
    tokio::fs::create_dir_all(&claude_dir).await?;
    let body = serde_json::json!({
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [{ "type": "command", "command": hook_path.to_string_lossy() }]
                },
                {
                    "matcher": "AskUserQuestion",
                    "hooks": [{ "type": "command", "command": hook_path.to_string_lossy() }]
                }
            ]
        }
    });
    let path = claude_dir.join("settings.local.json");
    tokio::fs::write(&path, serde_json::to_vec_pretty(&body)?).await?;
    Ok(())
}

/// Scan the newline-delimited stream-json output for the final
/// `{"type":"result", ...}` event and decode it as ClaudeOutput.
fn parse_stream_json_result(stdout: &str) -> Result<ClaudeOutput> {
    for line in stdout.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("type").and_then(|t| t.as_str()) == Some("result") {
            return serde_json::from_value::<ClaudeOutput>(v)
                .context("parsing result event");
        }
    }
    anyhow::bail!("no result event found in stream-json output")
}

fn tail(s: &str, n: usize) -> &str {
    if s.len() <= n {
        s
    } else {
        &s[s.len() - n..]
    }
}

enum Stream {
    Stdout,
    Stderr,
}

async fn stream_into_entry<R>(
    reader: R,
    entry: LiveEntry,
    which: Stream,
    mut session_tx: Option<tokio::sync::oneshot::Sender<String>>,
    mut budget: Option<(u64, tokio::sync::oneshot::Sender<u64>)>,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    let mut output_tokens: u64 = 0;
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let chunk = String::from_utf8_lossy(&buf).into_owned();
                {
                    let mut guard = entry.lock().await;
                    match which {
                        Stream::Stdout => guard.stdout.push_str(&chunk),
                        Stream::Stderr => guard.stderr.push_str(&chunk),
                    }
                }
                // Sniff session_id from the first stream-json line that has it.
                // The init event arrives within the first few lines; we send
                // it ASAP so a pause/kill still leaves something to resume from.
                if let Some(tx) = session_tx.take() {
                    match extract_session_id(chunk.trim()) {
                        Some(sid) => {
                            let _ = tx.send(sid);
                        }
                        None => session_tx = Some(tx),
                    }
                }
                // Track output tokens for budget abort.
                if let Some((limit, _)) = budget.as_ref() {
                    if let Some(delta) = extract_output_tokens(chunk.trim()) {
                        output_tokens = output_tokens.saturating_add(delta);
                        if output_tokens >= *limit {
                            if let Some((_, tx)) = budget.take() {
                                let _ = tx.send(output_tokens);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "reading process pipe");
                break;
            }
        }
    }
}

/// Pull `usage.output_tokens` from a single stream-json line, if present.
fn extract_output_tokens(line: &str) -> Option<u64> {
    if line.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let usage = v
        .get("usage")
        .or_else(|| v.get("message").and_then(|m| m.get("usage")))?;
    usage.get("output_tokens").and_then(|n| n.as_u64())
}

fn extract_session_id(line: &str) -> Option<String> {
    if line.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    v.get("session_id").and_then(|s| s.as_str()).map(String::from)
}

