use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::warn;
use uuid::Uuid;

use crate::agent::AgentBackend;
use crate::jobs::hub::LiveSessions;
use crate::jobs::output_log::LiveEntry;

pub fn tail(s: &str, n: usize) -> &str {
    if s.len() <= n {
        s
    } else {
        &s[s.len() - n..]
    }
}

pub enum Stream {
    Stdout,
    Stderr,
}

/// Pump a child process pipe into the live output entry, appending each line
/// and (for stdout) sniffing the session id and output-token usage via the
/// backend, and publishing each parsed event to the live hub for WebSocket
/// fan-out + persistence. The session id and budget signals are sent on their
/// oneshots the moment they're seen.
pub async fn stream_into_entry<R>(
    reader: R,
    entry: LiveEntry,
    which: Stream,
    backend: Arc<dyn AgentBackend>,
    hub: LiveSessions,
    task_id: Uuid,
    mut session_tx: Option<tokio::sync::oneshot::Sender<String>>,
    mut budget: Option<(u64, tokio::sync::oneshot::Sender<u64>)>,
    // Fires once per turn the moment a `result` event is seen, so the runner's
    // turn loop knows the current turn finished (and the agent is now idle).
    result_tx: Option<tokio::sync::mpsc::Sender<()>>,
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
                // Each stdout line is one agent event — fan it out live and
                // persist it. stderr is operational noise; it stays out of the
                // event stream (kept only in the output log for the error tail).
                if matches!(which, Stream::Stdout) {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(chunk.trim()) {
                        if value.is_object() {
                            let is_result =
                                value.get("type").and_then(|t| t.as_str()) == Some("result");
                            hub.publish_event(task_id, value).await;
                            if is_result {
                                if let Some(tx) = result_tx.as_ref() {
                                    let _ = tx.send(()).await;
                                }
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
