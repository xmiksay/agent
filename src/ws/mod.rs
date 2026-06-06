//! Single process-wide live WebSocket: `GET /ws?token=…`.
//!
//! One connection per browser multiplexes **every** task. Outbound it streams
//! [`Envelope`] frames (agent events, plus `auth_request`/`status` side-channels)
//! for all tasks; the client routes by `task_id`. Inbound it accepts operator
//! messages — chat, a goal redefinition, or stop — each naming its target task.
//!
//! Auth is a `?token=` query param checked in-handler (browsers can't set headers
//! on a WebSocket), so this route sits outside the `/api/*` bearer middleware.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

/// Operator → agent messages on the global socket — each carries the `task_id`,
/// since one connection multiplexes every task.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum InboundGlobal {
    Chat { task_id: Uuid, text: String },
    Redefine { task_id: Uuid, text: String },
    Stop { task_id: Uuid },
}

/// The client's first frame: the bearer token. Auth happens in-band (not via a
/// query param) so the token never lands in URLs/proxy logs.
#[derive(Deserialize)]
struct AuthFrame {
    #[serde(default)]
    token: Option<String>,
}

/// Single process-wide live stream: `GET /ws`. The upgrade is accepted
/// unconditionally; the client must send its token as the first message or the
/// server closes the connection. Then it streams every task's [`Envelope`]
/// frames (browser holds one connection, routes by `task_id`) and accepts
/// inbound operator messages that name their target task. A periodic ping keeps
/// idle intermediaries from dropping the socket.
pub async fn global_stream(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle_global(socket, state))
}

async fn handle_global(mut socket: WebSocket, state: AppState) {
    // In-band auth: the first frame carries the token. A mismatch closes.
    let presented = match socket.recv().await {
        Some(Ok(Message::Text(t))) => serde_json::from_str::<AuthFrame>(&t)
            .ok()
            .and_then(|a| a.token),
        _ => None,
    };
    if !crate::auth::token_ok(state.config.api_bearer_token.as_deref(), presented.as_deref()) {
        let _ = socket.send(Message::Close(None)).await;
        return;
    }

    let hub = state.task_store.hub().clone();
    let mut rx = hub.subscribe_all();
    let mut ping = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            ev = rx.recv() => match ev {
                Ok(env) => {
                    if send_envelope(&mut socket, &env).await.is_err() {
                        break;
                    }
                }
                // Skip dropped frames rather than tearing down every view's feed;
                // detail views refetch their own history via REST and dedupe by seq.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(text))) => handle_inbound_global(&hub, &text).await,
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(_)) => break,
            },
        }
    }
}

async fn handle_inbound_global(hub: &crate::jobs::hub::LiveSessions, text: &str) {
    let Ok(msg) = serde_json::from_str::<InboundGlobal>(text) else { return };
    match msg {
        InboundGlobal::Chat { task_id, text } => {
            hub.send_to_agent(task_id, &text).await;
        }
        InboundGlobal::Redefine { task_id, text } => {
            hub.send_to_agent(task_id, &format!("New goal: {text}")).await;
        }
        InboundGlobal::Stop { task_id } => {
            hub.stop(task_id).await;
        }
    }
}

async fn send_envelope(socket: &mut WebSocket, env: &crate::jobs::hub::Envelope) -> Result<(), ()> {
    let json = serde_json::to_string(env).map_err(|_| ())?;
    socket.send(Message::Text(json.into())).await.map_err(|_| ())
}
