use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::agent::resolve_backend;
use crate::auth::store::AuthStore;
use crate::auth::waiter::AuthWaiter;
use crate::config::Config;
use crate::jobs::hub::LiveSessions;
use crate::jobs::permission::{PermissionCtx, handle_permission};
use crate::jobs::prompt::build_prompt;
use crate::jobs::store::TaskStore;
use crate::jobs::stream::{PumpStreamCtx, Stream, pump_stream};
use crate::jobs::turn_kill::final_disposition;
use crate::jobs::types::TriggerReason;
use crate::models::ResolvedModel;
use crate::project::{BranchStatus, EnvContext, NewBranchEntry, ProjectStore, ProviderKind};
use crate::provider::{GitProvider, resolve_token};
use crate::service::Service;
use crate::workspace::Workspace;
use crate::workspace::git::HttpsAuth;
use crate::workspace::layout::slugify;

/// Everything `run_job` needs for one interactive agent session. Grouped so the
/// runner is driven by a single owned value rather than a twenty-deep positional
/// argument list assembled at the call site (`confirm_task`).
pub struct RunJobContext {
    pub task_id: uuid::Uuid,
    pub trigger: TriggerReason,
    pub service: Service,
    pub project_id: Option<uuid::Uuid>,
    pub git_url: String,
    pub project_path: String,
    pub default_branch: String,
    pub branch_override: Option<String>,
    pub config: Config,
    pub provider: Arc<dyn GitProvider>,
    pub workspace: Arc<Workspace>,
    pub project_store: Arc<ProjectStore>,
    pub hub: LiveSessions,
    pub store: Arc<TaskStore>,
    pub auth_store: Arc<AuthStore>,
    pub auth_waiter: AuthWaiter,
    pub semaphore: Arc<Semaphore>,
    pub resume_session_id: Option<String>,
    pub prompt_override: Option<String>,
    pub model: Option<ResolvedModel>,
}

pub async fn run_job(ctx: RunJobContext) -> Result<()> {
    let RunJobContext {
        task_id,
        trigger,
        service,
        project_id,
        git_url,
        project_path,
        default_branch,
        branch_override,
        config,
        provider,
        workspace,
        project_store,
        hub,
        store,
        auth_store,
        auth_waiter,
        semaphore,
        resume_session_id,
        prompt_override,
        model,
    } = ctx;

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

    // Resolve the per-service token for the session-start clone/fetch and the
    // agent's spawn env. We keep the credential *source* (`provider_creds`) too,
    // so the runner can re-resolve just-in-time before every turn's push: a long
    // session outlives a GitHub App installation token (~1h TTL), and
    // `resolve_token` consults the refreshing cache (provider::github::app), so a
    // captured-once string would push with a dead token (#44).
    let provider_creds = service.credentials()?;
    let provider_token_value = resolve_token(&provider_creds).await?;
    let provider_token_var = match service.kind {
        ProviderKind::Github => "GH_TOKEN",
        ProviderKind::Gitlab => "GITLAB_TOKEN",
    };

    // git_url is the project's remote — an SSH (git@host:path.git) or HTTPS URL,
    // from the webhook normalizers or operator-supplied at manual creation. Either
    // way we derive a token-HTTPS remote from it so clone/push need no host SSH key
    // and no secret is written into .git/config.
    let https_auth = HttpsAuth::from_remote_url(&git_url, service.kind, &provider_token_value)?;
    // A ReviewMR worktree is a scratch `<source>-review` branch that must carry the
    // MR's source content, so it's created from the source branch; every other
    // trigger bases its branch on the project default.
    let base_branch = match &trigger {
        TriggerReason::ReviewMR { source_branch, .. } => source_branch.clone(),
        _ => default_branch.clone(),
    };
    workspace
        .clone_or_fetch(&work_dir, &https_auth, &branch, &base_branch)
        .await?;

    // The agent's *own* git/gh/glab calls read the token from a mutable env file
    // it re-sources per command (BASH_ENV), not the frozen spawn-time process
    // env — so the token can be rotated mid-session (issue #52). Seed it now with
    // the freshly-resolved token; `.git/` exists after the clone/fetch above.
    crate::workspace::write_agent_env(&work_dir, provider_token_var, &provider_token_value)
        .await
        .context("seeding agent.env")?;
    let agent_env_path = crate::workspace::agent_env_path(&work_dir);

    // The selected model's provider picks the backend/CLI; `model_arg` is its
    // `model_id` (None → default backend + the CLI's own default model).
    let (backend, model_arg) = resolve_backend(model.as_ref())?;

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

    // Per-task throwaway PostgreSQL (issue #26). Provision now — before spawning
    // the agent — so a provision failure aborts cleanly. The guard tears the
    // role+DB down on every exit path (`?`, graceful end, abort); the startup
    // sweep is the backstop if a hard SIGKILL skips Drop.
    let db_guard = match config.project_db_admin_url.as_deref() {
        Some(admin_url) => {
            let host = crate::jobs::project_db::agent_host_from_admin(
                admin_url,
                config.project_db_host_for_agent.as_deref(),
            );
            let pdb = crate::jobs::project_db::ProjectDb::provision(admin_url, &host, task_id)
                .await
                .context("provisioning per-task project database")?;
            crate::jobs::project_db::ProjectDbGuard(Some(pdb))
        }
        None => crate::jobs::project_db::ProjectDbGuard(None),
    };
    let db_note = db_guard.0.is_some();

    let prompt = match prompt_override {
        Some(p) if !p.trim().is_empty() => p,
        _ => build_prompt(&trigger, &branch, &default_branch, service.kind, db_note),
    };
    info!(%prompt, program = backend.program(), model = ?model_arg, "running agent");

    // DANGEROUS when set: an `unbound` model runs with no permission gating.
    let unbound = model.as_ref().is_some_and(|m| m.unbound);
    if unbound {
        warn!(%task_id, model = ?model_arg, "running UNBOUND: all tool calls allowed without approval");
    }
    let agent_args =
        backend.build_args(resume_session_id.as_deref(), model_arg.as_deref(), unbound);

    // `gh`/`glab` and the agent's own `git push` inside the worktree authenticate
    // against the same token (already resolved above for git transport). The env
    // layering + spawn lives in `jobs::spawn` to keep the turn loop under the cap.
    let env_ctx = EnvContext {
        branch: branch.clone(),
        default_branch: default_branch.clone(),
        url: git_url.clone(),
        project: project_path.clone(),
        service: service.slug.clone(),
        task_id: task_id.to_string(),
    };
    let mut child = crate::jobs::spawn::spawn_agent(crate::jobs::spawn::SpawnAgentCtx {
        backend: &backend,
        agent_args: &agent_args,
        work_dir: &work_dir,
        project_id,
        project_store: &project_store,
        env_ctx,
        model: model.as_ref(),
        db_guard: &db_guard,
        config: &config,
        provider_token_var,
        provider_token_value: &provider_token_value,
        agent_env_path: &agent_env_path,
    })
    .await?;

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
    hub.register(task_id, backend.clone(), input_tx, to_agent_tx.clone())
        .await;

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
                let _ = store.set_session_id(task_id, &sid).await;
            }
        });
    }

    // Token-budget abort: fired once cumulative output tokens cross 50% of the
    // budget; we SIGKILL and end the session (session_id is captured, so Resume
    // works after a rate-limit reset).
    let (budget_tx, budget_rx) = tokio::sync::oneshot::channel::<u64>();
    let token_limit = config.task_token_budget / 2;

    // Carries the turn's `result` event, the moment it's seen on stdout.
    let (result_tx, mut result_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(8);

    // Permission prompts (`can_use_tool`) sniffed off stdout. The sender is owned
    // ONLY by the stdout reader so it drops at stdout EOF and ends the consumer.
    let (perm_tx, mut perm_rx) = tokio::sync::mpsc::channel::<crate::agent::PermissionRequest>(32);
    let perm_ctx = PermissionCtx {
        task_id,
        project_id,
        hub: hub.clone(),
        auth_store: auth_store.clone(),
        auth_waiter: auth_waiter.clone(),
        project_store: project_store.clone(),
        approval_timeout_secs: config.operator_approval_timeout_secs,
    };
    let perm_consumer = tokio::spawn(async move {
        // One task per request so a long operator wait never blocks the next.
        while let Some(req) = perm_rx.recv().await {
            tokio::spawn(handle_permission(req, perm_ctx.clone()));
        }
    });

    let stdout_reader = tokio::spawn(pump_stream(
        stdout_pipe,
        Stream::Stdout,
        PumpStreamCtx {
            backend: backend.clone(),
            hub: hub.clone(),
            task_id,
            session_tx: Some(session_tx),
            budget: Some((token_limit, budget_tx)),
            result_tx: Some(result_tx),
            perm_tx: Some(perm_tx),
        },
    ));
    let stderr_reader = tokio::spawn(pump_stream(
        stderr_pipe,
        Stream::Stderr,
        PumpStreamCtx {
            backend: backend.clone(),
            hub: hub.clone(),
            task_id,
            session_tx: None,
            budget: None,
            result_tx: None,
            perm_tx: None,
        },
    ));

    let code_trigger = matches!(
        trigger,
        TriggerReason::Issue { .. }
            | TriggerReason::FixReview { .. }
            | TriggerReason::MRComment { .. }
            | TriggerReason::IssueComment { .. }
    );
    let work_dir_str = work_dir.to_string_lossy().into_owned();

    let (killed_for_budget, killed_for_timeout) = crate::jobs::turn_loop::run_turn_loop(
        crate::jobs::turn_loop::TurnLoopCtx {
            backend: &backend,
            semaphore: &semaphore,
            hub: &hub,
            store: &store,
            trigger: &trigger,
            provider: provider.as_ref(),
            provider_creds: &provider_creds,
            provider_token_var,
            project_path: &project_path,
            work_dir: &work_dir,
            work_dir_str: &work_dir_str,
            task_id,
            code_trigger,
            token_limit,
            job_timeout_secs: config.job_timeout_secs,
        },
        &mut child,
        &prompt,
        &mut input_rx,
        &to_agent_tx,
        &mut result_rx,
        budget_rx,
    )
    .await;

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

    // Final disposition from the child exit code. Operator Pause aborts this
    // runner task before reaching here (kill_task records "paused"), so the
    // exit-code path covers natural exits / crashes / budget or per-turn-timeout
    // kill; a graceful Stop makes claude exit 0 → completed. `unwrap_or(true)`: an
    // unreadable status must not falsely mark the task failed.
    //
    // `killed` is no longer a state — the reason is recorded as a result note,
    // and the durable axes carry the verdict (cold/completed on success,
    // failed/failed otherwise).
    let exit_ok = exit_status.map(|s| s.success()).unwrap_or(true);
    let (agent_state, task_state, note) =
        final_disposition(killed_for_timeout, killed_for_budget, exit_ok);
    hub.mark_idle(task_id);
    let _ = store
        .finish_task(task_id, agent_state, task_state, note)
        .await;
    info!(%task_id, agent_state, task_state, "agent session ended");
    Ok(())
}
