use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{info, warn};
use tokio::sync::Semaphore;

use crate::agent::{AgentBackend, ClaudeCode};
use crate::config::Config;
use crate::git_service::GitService;
use crate::jobs::hub::LiveSessions;
use crate::jobs::output_log::TaskOutputLog;
use crate::jobs::prompt::build_prompt;
use crate::jobs::store::TaskStore;
use crate::jobs::stream::{Stream, stream_into_entry};
use crate::jobs::types::TriggerReason;
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
    store: Arc<TaskStore>,
    semaphore: Arc<Semaphore>,
    resume_session_id: Option<String>,
    prompt_override: Option<String>,
) -> Result<()> {
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
        let _ = store.set_pid(task_id, Some(pid)).await;
    }

    // The turn loop writes one operator message per turn directly to stdin,
    // gated by a semaphore permit so only an *actively-processing* agent counts
    // against MAX_CONCURRENT_JOBS — an idle, warm agent holds no slot. The hub
    // holds the input sender; dropping it (Stop/end) closes the input channel,
    // which the loop reads as a graceful close (stdin EOF → claude exits).
    let mut child_stdin = child.stdin.take().expect("piped stdin");
    let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<String>(32);
    hub.register(task_id, backend.clone(), input_tx).await;

    let stdout_pipe = child.stdout.take().expect("piped stdout");
    let stderr_pipe = child.stderr.take().expect("piped stderr");

    // Persist the session id the moment the stream reader sniffs it (first turn),
    // so a later resume works even if the agent is killed.
    let (session_tx, session_rx) = tokio::sync::oneshot::channel::<String>();
    {
        let store = store.clone();
        tokio::spawn(async move {
            if let Ok(sid) = session_rx.await {
                let _ = store.set_session_id_pub(task_id, &sid).await;
            }
        });
    }

    // Token-budget abort: fired once cumulative output tokens cross 50% of the
    // budget; we SIGKILL and end the session (session_id is captured, so Resume
    // works after a rate-limit reset).
    let (budget_tx, mut budget_rx) = tokio::sync::oneshot::channel::<u64>();
    let token_limit = config.task_token_budget / 2;

    // One signal per turn, the moment a `result` event is seen on stdout.
    let (result_tx, mut result_rx) = tokio::sync::mpsc::channel::<()>(8);

    let stdout_reader = tokio::spawn(stream_into_entry(
        stdout_pipe,
        entry.clone(),
        Stream::Stdout,
        backend.clone(),
        hub.clone(),
        task_id,
        Some(session_tx),
        Some((token_limit, budget_tx)),
        Some(result_tx),
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
        None,
    ));

    let code_trigger = matches!(
        trigger,
        TriggerReason::Issue { .. }
            | TriggerReason::FixReview { .. }
            | TriggerReason::MRComment { .. }
            | TriggerReason::IssueComment { .. }
    );
    let work_dir_str = work_dir.to_string_lossy().into_owned();

    // Turn loop. The first turn carries the initial prompt; later turns pull an
    // operator message from the hub. A turn = acquire permit → write message →
    // wait for its `result` → release permit → finalize (push + reply on demand)
    // → go idle (warm, holding no slot) until the next message or a graceful
    // close. No per-turn timeout — Stop/Pause from the UI handle a stuck turn.
    let mut pending = Some(backend.encode_user_message(&prompt));
    let mut killed_for_budget = false;
    loop {
        let msg = match pending.take() {
            Some(m) => m,
            None => {
                let _ = store.set_status(task_id, "completed").await;
                tokio::select! {
                    m = input_rx.recv() => match m {
                        Some(m) => m,
                        None => break, // Stop: the hub dropped the input sender
                    },
                    _ = child.wait() => break,
                }
            }
        };

        // A turn begins — only now do we occupy a concurrency slot.
        let permit = match semaphore.acquire().await {
            Ok(p) => p,
            Err(_) => break,
        };
        let _ = store.set_status(task_id, "running").await;

        if child_stdin.write_all(msg.as_bytes()).await.is_err()
            || child_stdin.write_all(b"\n").await.is_err()
            || child_stdin.flush().await.is_err()
        {
            drop(permit);
            break;
        }

        let mut turn_exited = false;
        tokio::select! {
            _ = result_rx.recv() => {} // this turn finished
            _ = child.wait() => turn_exited = true,
            used = &mut budget_rx => {
                let used = used.unwrap_or_default();
                warn!(%task_id, used, limit = token_limit, "token budget reached, killing claude");
                let _ = child.start_kill();
                let _ = child.wait().await;
                turn_exited = true;
                killed_for_budget = true;
            }
        }
        drop(permit); // released between turns — an idle agent holds no slot

        crate::jobs::turn::finalize_turn(
            &entry,
            backend.as_ref(),
            &store,
            &trigger,
            &project_path,
            provider.as_ref(),
            &work_dir_str,
            task_id,
            code_trigger,
        )
        .await;

        if turn_exited {
            break;
        }
    }

    // Session over: close stdin (EOF), reap the child, end the live session and
    // drain the readers (the pipes hit EOF once the child is gone).
    drop(child_stdin);
    let _ = child.wait().await;
    hub.end(task_id).await;
    let _ = stdout_reader.await;
    let _ = stderr_reader.await;
    {
        let mut guard = entry.lock().await;
        guard.finished = true;
    }

    let final_status = if killed_for_budget { "killed" } else { "completed" };
    let _ = store.finish_task(task_id, final_status).await;
    info!(%task_id, status = final_status, "agent session ended");
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

