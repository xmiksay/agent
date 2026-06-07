use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{info, warn};
use tokio::sync::Semaphore;

use crate::agent::{AgentBackend, ClaudeCode};
use crate::auth::store::AuthStore;
use crate::auth::waiter::AuthWaiter;
use crate::config::Config;
use crate::git_service::GitService;
use crate::jobs::hub::LiveSessions;
use crate::jobs::permission::handle_permission;
use crate::jobs::prompt::build_prompt;
use crate::jobs::store::TaskStore;
use crate::jobs::stream::{Stream, pump_stream};
use crate::jobs::types::TriggerReason;
use crate::project::{
    BranchStatus, EnvContext, NewBranchEntry, ProjectStore, ProviderKind, build_env_vars,
};
use crate::provider::GitProvider;
use crate::workspace::Workspace;
use crate::workspace::layout::slugify;

#[allow(clippy::too_many_arguments)]
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
    hub: LiveSessions,
    store: Arc<TaskStore>,
    auth_store: Arc<AuthStore>,
    auth_waiter: AuthWaiter,
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
        _ => build_prompt(&trigger, &branch, service.kind),
    };
    info!(%prompt, program = backend.program(), "running agent");

    let agent_args = backend.build_args(resume_session_id.as_deref());

    // So `gh`/`glab` inside the worktree authenticate against the same PAT the
    // agent uses to clone and post notes — picked by provider kind.
    let (provider_token_var, provider_token_value) = match service.kind {
        ProviderKind::Github => ("GH_TOKEN", service.token.clone()),
        ProviderKind::Gitlab => ("GITLAB_TOKEN", service.token.clone()),
    };

    let mut cmd = Command::new(backend.program());
    cmd.args(&agent_args).current_dir(&work_dir);
    // Project-configured env first, so reserved vars below always win. The stored
    // value is a minijinja template rendered against the task's runtime vars.
    if let Some(pid) = project_id
        && let Ok(Some(pc)) = project_store.get_project_by_id(pid).await
    {
        let ctx = EnvContext {
            branch: branch.clone(),
            default_branch: default_branch.clone(),
            url: git_url.clone(),
            project: project_path.clone(),
            service: service.slug.clone(),
            task_id: task_id.to_string(),
        };
        match build_env_vars(&pc.env_file, &ctx) {
            Ok(pairs) => {
                for (key, value) in pairs {
                    cmd.env(key, value);
                }
            }
            Err(e) => {
                warn!(%task_id, project = %project_path, error = %e, "skipping project env: template error")
            }
        }
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

    // Two stdin channels feed a single writer task that owns child stdin:
    //   * `input` carries operator messages; the turn loop drains it one per
    //     turn for pacing, then forwards each into `to_agent`.
    //   * `to_agent` carries raw lines (operator messages AND control responses)
    //     to the writer. The hub holds a `to_agent` clone so a mid-turn
    //     `can_use_tool` response reaches stdin immediately, not at turn end.
    // When every `to_agent` sender drops, the writer ends and dropping child
    // stdin produces EOF — the graceful-close mechanism.
    let child_stdin = child.stdin.take().expect("piped stdin");
    let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<String>(32);
    let (to_agent_tx, mut to_agent_rx) = tokio::sync::mpsc::channel::<String>(32);
    hub.register(task_id, backend.clone(), input_tx, to_agent_tx.clone()).await;

    let writer = tokio::spawn(async move {
        let mut child_stdin = child_stdin;
        while let Some(line) = to_agent_rx.recv().await {
            if child_stdin.write_all(line.as_bytes()).await.is_err()
                || child_stdin.write_all(b"\n").await.is_err()
                || child_stdin.flush().await.is_err()
            {
                break;
            }
        }
        // Dropping child_stdin here closes the pipe → EOF → claude exits.
    });

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

    // Carries the turn's `result` event, the moment it's seen on stdout.
    let (result_tx, mut result_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(8);

    // Permission prompts (`can_use_tool`) sniffed off stdout. The sender is owned
    // ONLY by the stdout reader so it drops at stdout EOF and ends the consumer.
    let (perm_tx, mut perm_rx) =
        tokio::sync::mpsc::channel::<crate::agent::PermissionRequest>(32);
    let perm_consumer = {
        let hub = hub.clone();
        let auth_store = auth_store.clone();
        let auth_waiter = auth_waiter.clone();
        let project_store = project_store.clone();
        tokio::spawn(async move {
            // One task per request so a 600s operator wait never blocks the next.
            while let Some(req) = perm_rx.recv().await {
                tokio::spawn(handle_permission(
                    req,
                    task_id,
                    project_id,
                    hub.clone(),
                    auth_store.clone(),
                    auth_waiter.clone(),
                    project_store.clone(),
                ));
            }
        })
    };

    let stdout_reader = tokio::spawn(pump_stream(
        stdout_pipe,
        Stream::Stdout,
        backend.clone(),
        hub.clone(),
        task_id,
        Some(session_tx),
        Some((token_limit, budget_tx)),
        Some(result_tx),
        Some(perm_tx),
    ));
    let stderr_reader = tokio::spawn(pump_stream(
        stderr_pipe,
        Stream::Stderr,
        backend.clone(),
        hub.clone(),
        task_id,
        None,
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
    // operator message from the hub. A turn = acquire permit → forward message to
    // the writer → wait for its `result` → release permit → finalize (push +
    // reply on demand) → go idle (warm, holding no slot) until the next message
    // or a graceful close. No per-turn timeout — Stop/Pause from the UI handle a
    // stuck turn.
    let mut pending = Some(backend.encode_user_message(&prompt));
    let mut killed_for_budget = false;
    let mut last_result: Option<serde_json::Value> = None;
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

        // Forward to the writer task (which owns child stdin). A send error means
        // the writer is gone (stdin closed) — end the session.
        if to_agent_tx.send(msg).await.is_err() {
            drop(permit);
            break;
        }

        let mut turn_exited = false;
        tokio::select! {
            r = result_rx.recv() => last_result = r, // this turn finished
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

        // Only finalize when the turn produced a result event; a turn that ended
        // via child exit/budget before its result has nothing to parse.
        if let Some(rv) = last_result.take() {
            crate::jobs::turn::finalize_turn(
                rv,
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
        }

        if turn_exited {
            break;
        }
    }

    // Session over. Drop this clone of `to_agent`, then end the live session
    // (which drops the hub's input + control clones). With every `to_agent`
    // sender gone the writer drains and drops child stdin → EOF; reap the child
    // and drain the readers (the pipes hit EOF once the child is gone). The
    // stdout reader owns `perm_tx`, so awaiting it ends the permission consumer.
    drop(to_agent_tx);
    hub.end(task_id).await;
    let _ = writer.await;
    let exit_status = child.wait().await.ok();
    let _ = stdout_reader.await;
    let _ = stderr_reader.await;
    let _ = perm_consumer.await;

    // Final status from the child exit code. Operator Pause aborts this runner
    // task before reaching here (kill_task sets the status), so exit-code→failed
    // applies only to natural exits / crashes; a graceful Stop makes claude exit
    // 0 → completed. `unwrap_or(true)`: an unreadable status must not falsely
    // mark the task failed.
    let final_status = if killed_for_budget {
        "killed"
    } else if exit_status.map(|s| s.success()).unwrap_or(true) {
        "completed"
    } else {
        "failed"
    };
    let _ = store.finish_task(task_id, final_status).await;
    info!(%task_id, status = final_status, "agent session ended");
    Ok(())
}
