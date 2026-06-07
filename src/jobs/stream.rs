use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::warn;
use uuid::Uuid;

use crate::agent::{AgentBackend, PermissionRequest};
use crate::jobs::hub::LiveSessions;

pub enum Stream {
    Stdout,
    Stderr,
}

/// Pump a child process pipe. For stdout: parse each line into an agent event,
/// publish it to the live hub for WebSocket fan-out + persistence, sniff the
/// session id and output-token usage via the backend, route `can_use_tool`
/// control requests to the permission handler, and signal the per-turn `result`.
/// For stderr: drain to EOF and log each non-empty line at debug level.
///
/// The session id and budget signals are sent on their oneshots the moment
/// they're seen.
///
/// `perm_tx` (stdout only) receives any `can_use_tool` control request parsed
/// off the stream; those lines are internal plumbing and are NOT published as
/// timeline events (the operator sees them via the auth_request side-channel).
#[allow(clippy::too_many_arguments)]
pub async fn pump_stream<R>(
    reader: R,
    which: Stream,
    backend: Arc<dyn AgentBackend>,
    hub: LiveSessions,
    task_id: Uuid,
    mut session_tx: Option<tokio::sync::oneshot::Sender<String>>,
    mut budget: Option<(u64, tokio::sync::oneshot::Sender<u64>)>,
    // Carries the `result` event the moment it's seen, so the runner's turn loop
    // knows the current turn finished (and the agent is now idle) and can
    // finalize from it.
    result_tx: Option<tokio::sync::mpsc::Sender<serde_json::Value>>,
    perm_tx: Option<tokio::sync::mpsc::Sender<PermissionRequest>>,
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
                // stderr is operational noise: it never enters the event stream.
                // Drain it so the pipe doesn't block, and log it for diagnostics.
                if matches!(which, Stream::Stderr) {
                    let line = chunk.trim();
                    if !line.is_empty() {
                        tracing::debug!(%task_id, line, "agent stderr");
                    }
                    continue;
                }
                // A `can_use_tool` control request is internal plumbing: route it
                // to the permission handler and skip publishing it as a timeline
                // event. Session-id/token sniffing below is irrelevant for it.
                if let Some(tx) = perm_tx.as_ref()
                    && let Some(req) = backend.parse_permission_request(chunk.trim())
                {
                    let _ = tx.send(req).await;
                    continue;
                }
                // Each stdout line is one agent event — fan it out live and
                // persist it. When it's the turn's `result`, hand it to the
                // runner so it can finalize from the event itself.
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(chunk.trim()) {
                    if value.is_object() {
                        let is_result =
                            value.get("type").and_then(|t| t.as_str()) == Some("result");
                        hub.publish_event(task_id, value.clone()).await;
                        if is_result {
                            if let Some(tx) = result_tx.as_ref() {
                                let _ = tx.send(value).await;
                            }
                        }
                    }
                }
                // Sniff session_id from the first stream line that has it. The
                // init event arrives within the first few lines; we send it ASAP
                // so a pause/kill still leaves something to resume from.
                if let Some(tx) = session_tx.take() {
                    match backend.extract_session_id(chunk.trim()) {
                        Some(sid) => {
                            let _ = tx.send(sid);
                        }
                        None => session_tx = Some(tx),
                    }
                }
                // Track output tokens for budget abort.
                if let Some((limit, _)) = budget.as_ref() {
                    if let Some(delta) = backend.extract_output_tokens(chunk.trim()) {
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
