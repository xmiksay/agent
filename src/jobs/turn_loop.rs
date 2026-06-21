//! The runner's turn loop. The first turn carries the initial prompt; later
//! turns pull an operator message from the hub. A turn = acquire permit → forward
//! message to the writer → wait for its `result` → release permit → finalize
//! (push + reply on demand) → go idle (warm, holding no slot) until the next
//! message or a graceful close. Split out of `run_job` to keep the runner under
//! the file cap; channel wiring and final disposition stay in `runner`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::process::Child;
use tokio::sync::{Semaphore, mpsc, oneshot};
use tracing::warn;
use uuid::Uuid;

use crate::agent::AgentBackend;
use crate::jobs::hub::LiveSessions;
use crate::jobs::store::TaskStore;
use crate::jobs::turn::{FinalizeTurnCtx, finalize_turn};
use crate::jobs::turn_kill::kill_process_group;
use crate::jobs::types::TriggerReason;
use crate::provider::{GitProvider, resolve_token};
use crate::service::ServiceCredentials;

/// Inputs the turn loop needs from the runner that don't change between turns.
/// Grouped to keep the call signature manageable.
pub(crate) struct TurnLoopCtx<'a> {
    pub backend: &'a Arc<dyn AgentBackend>,
    pub semaphore: &'a Arc<Semaphore>,
    pub hub: &'a LiveSessions,
    pub store: &'a Arc<TaskStore>,
    pub trigger: &'a TriggerReason,
    pub provider: &'a dyn GitProvider,
    pub provider_creds: &'a ServiceCredentials,
    pub provider_token_var: &'a str,
    pub project_path: &'a str,
    pub work_dir: &'a Path,
    pub work_dir_str: &'a str,
    pub task_id: Uuid,
    pub code_trigger: bool,
    pub token_limit: u64,
    pub job_timeout_secs: u64,
}

/// Drive the per-turn loop until the session ends. Returns
/// `(killed_for_budget, killed_for_timeout)` for the caller's final disposition.
pub(crate) async fn run_turn_loop(
    ctx: TurnLoopCtx<'_>,
    child: &mut Child,
    prompt: &str,
    input_rx: &mut mpsc::Receiver<String>,
    to_agent_tx: &mpsc::Sender<String>,
    result_rx: &mut mpsc::Receiver<serde_json::Value>,
    mut budget_rx: oneshot::Receiver<u64>,
) -> (bool, bool) {
    let TurnLoopCtx {
        backend,
        semaphore,
        hub,
        store,
        trigger,
        provider,
        provider_creds,
        provider_token_var,
        project_path,
        work_dir,
        work_dir_str,
        task_id,
        code_trigger,
        token_limit,
        job_timeout_secs,
    } = ctx;

    let mut pending = Some(backend.encode_user_message(prompt));
    let mut killed_for_budget = false;
    let mut killed_for_timeout = false;
    let mut last_result: Option<serde_json::Value> = None;
    loop {
        let msg = match pending.take() {
            Some(m) => m,
            None => {
                // Turn done → go warm-idle: clear the active-turn flag and mark
                // the lifecycle completed. Durable agent_state is written `cold`
                // (so a crash while warm leaves a resumable row, not a stuck
                // `pending` one); the task can still resume, so no finished_at.
                hub.mark_idle(task_id);
                let _ = store.set_states(task_id, "cold", "completed").await;
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
        // A turn is actively processing: flag it in the hub (overlays as the
        // derived `running` agent_state) and advance the lifecycle to working_on.
        // Durable agent_state is written `cold` (warm/running is never persisted),
        // overwriting the `pending` confirm left behind so a crash mid-run leaves
        // a resumable row rather than a stuck `pending` one.
        hub.mark_running(task_id);
        let _ = store.set_states(task_id, "cold", "working_on").await;

        // Refresh agent.env at the start of each turn so a warm-idle wake-up past
        // the App token's ~1h TTL runs the agent's own git/gh/glab with a live
        // token (#52). Re-resolving is cheap for a PAT and hits the refreshing
        // cache for an App; best-effort — a write failure must not fail the turn.
        match resolve_token(provider_creds).await {
            Ok(token) => {
                if let Err(e) =
                    crate::workspace::write_agent_env(work_dir, provider_token_var, &token).await
                {
                    warn!(%task_id, error = %e, "failed to refresh agent.env for turn");
                }
            }
            Err(e) => warn!(%task_id, error = %e, "failed to resolve token for turn refresh"),
        }

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
            // Per-turn watchdog. Disabled when `job_timeout_secs == 0`; otherwise a
            // turn that runs past the limit (e.g. wedged in a shell poll loop) gets
            // the whole agent process group SIGKILLed and the task left resumable.
            _ = tokio::time::sleep(Duration::from_secs(job_timeout_secs)),
                if job_timeout_secs > 0 =>
            {
                warn!(%task_id, limit_secs = job_timeout_secs, "per-turn timeout exceeded, killing agent subtree");
                kill_process_group(child).await;
                turn_exited = true;
                killed_for_timeout = true;
            }
        }
        drop(permit); // released between turns — an idle agent holds no slot

        // Only finalize when the turn produced a result event; a turn that ended
        // via child exit/budget before its result has nothing to parse.
        if let Some(rv) = last_result.take() {
            finalize_turn(
                rv,
                FinalizeTurnCtx {
                    backend: backend.as_ref(),
                    store,
                    trigger,
                    project_path,
                    provider,
                    work_dir: work_dir_str,
                    task_id,
                    code_trigger,
                    token_env: provider_token_var,
                    creds: provider_creds,
                },
            )
            .await;
        }

        if turn_exited {
            break;
        }
    }

    (killed_for_budget, killed_for_timeout)
}
