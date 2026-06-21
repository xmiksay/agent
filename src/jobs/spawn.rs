//! Build the agent CLI command — layering project/model/per-task-DB/NVM env in a
//! fixed precedence — and spawn it. Split out of `run_job` to keep the runner's
//! turn loop under the file cap; the wiring of stdin/stdout and the loop itself
//! stay in `runner`.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::process::{Child, Command};
use tracing::info;
use uuid::Uuid;

use crate::agent::AgentBackend;
use crate::config::Config;
use crate::jobs::project_db::ProjectDbGuard;
use crate::models::ResolvedModel;
use crate::project::{EnvContext, ProjectStore};

/// Inputs for spawning the agent process. Grouped so `spawn_agent` takes one
/// borrowed context instead of a twelve-deep argument list.
pub(crate) struct SpawnAgentCtx<'a> {
    pub backend: &'a Arc<dyn AgentBackend>,
    pub agent_args: &'a [String],
    pub work_dir: &'a Path,
    pub project_id: Option<Uuid>,
    pub project_store: &'a Arc<ProjectStore>,
    pub env_ctx: EnvContext,
    pub model: Option<&'a ResolvedModel>,
    pub db_guard: &'a ProjectDbGuard,
    pub config: &'a Config,
    pub provider_token_var: &'a str,
    pub provider_token_value: &'a str,
    pub agent_env_path: &'a Path,
}

/// Construct the agent process command with its full environment and spawn it
/// (stdin/stdout/stderr piped, own process group). Env precedence, lowest first:
/// project-configured env, then model provider env, then the per-task DB vars,
/// then the provider token + `BASH_ENV` — so reserved vars always win over a
/// project's template.
pub(crate) async fn spawn_agent(ctx: SpawnAgentCtx<'_>) -> Result<Child> {
    let SpawnAgentCtx {
        backend,
        agent_args,
        work_dir,
        project_id,
        project_store,
        env_ctx,
        model,
        db_guard,
        config,
        provider_token_var,
        provider_token_value,
        agent_env_path,
    } = ctx;

    let mut cmd = Command::new(backend.program());
    cmd.args(agent_args).current_dir(work_dir);
    // Project-configured env first, so reserved vars below always win. The stored
    // value is a minijinja template rendered against the task's runtime vars.
    if let Some(pid) = project_id {
        for (key, value) in project_store.spawn_env(pid, &env_ctx).await {
            cmd.env(key, value);
        }
    }
    // Provider API key + base URL (API mode) after project env so a project can't
    // clobber them; absent, the CLI runs on its subscription login + default host.
    crate::agent::apply_model_env(&mut cmd, backend.as_ref(), model);

    // Per-task DB connection (issue #26), after project env so it always wins.
    // `DATABASE_URL` carries the password; the `PG*` vars let bare `psql` connect.
    if let Some(pdb) = db_guard.0.as_ref() {
        cmd.env("DATABASE_URL", &pdb.agent_url);
        cmd.env("PGHOST", pdb.host());
        if let Some(port) = pdb.port() {
            cmd.env("PGPORT", port);
        }
        cmd.env("PGDATABASE", pdb.name());
        cmd.env("PGUSER", pdb.name());
        cmd.env("PGPASSWORD", pdb.password());
    }

    // NVM (issue #26): resolve the node toolchain NVM would activate (honouring
    // the worktree's `.nvmrc`) and prepend its `bin` to the child's PATH. The
    // child inherits this process's env by default, so we only prepend.
    if let Some(nvm_dir) = config.nvm_dir.as_deref()
        && let Some(bin) =
            crate::jobs::nvm::resolve_node_bin(std::path::Path::new(nvm_dir), work_dir).await
    {
        let existing = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}:{existing}", bin.display()));
        info!(node_bin = %bin.display(), "running agent inside NVM environment");
    }

    let child = cmd
        .env(provider_token_var, provider_token_value)
        // Bash sources $BASH_ENV at the start of every non-interactive shell (how
        // the CLI's Bash tool runs commands), so each git/gh/glab invocation
        // re-reads the *current* token from agent.env. The frozen process env var
        // above is the belt-and-suspenders initial value; the sourced file wins.
        .env("BASH_ENV", agent_env_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        // Own process group so the per-turn timeout can SIGKILL the whole subtree
        // (`kill -pgid`), not just the CLI — backgrounded test processes the agent
        // spawned must die with it. `kill_on_drop` alone would leak them.
        .process_group(0)
        .spawn()
        .context("failed to spawn agent")?;

    Ok(child)
}
