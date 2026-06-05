//! Live task WebSocket: `GET /ws/tasks/{id}?token=…`.
//!
//! Outbound, the socket streams [`Envelope`] frames (agent events, plus
//! `auth_request`/`status` side-channels) from the task's hub channel. Inbound,
//! it accepts operator messages — chat, a goal redefinition, or stop — and routes
//! them to the running agent's stdin via the hub.
//!
//! Auth is a `?token=` query param checked in-handler (browsers can't set headers
//! on a WebSocket), so this route sits outside the `/api/*` bearer middleware.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

#[derive(Deserialize)]
pub struct TokenQuery {
    token: Option<String>,
}

/// Operator → agent messages.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Inbound {
    /// A chat turn for the running agent.
    Chat { text: String },
    /// Redirect the agent's goal (sent immediately as a framed user message).
    Redefine { text: String },
    /// Graceful stop: close the agent's stdin so it wraps up the current turn.
    Stop,
}

pub async fn task_stream(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<TokenQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    if !crate::auth::token_ok(state.config.api_bearer_token.as_deref(), q.token.as_deref()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, id)))
}

async fn handle_socket(mut socket: WebSocket, state: AppState, task_id: Uuid) {
    let hub = state.task_store.hub().clone();
    let (snapshot, mut rx) = hub.subscribe(task_id).await;

    // Replay the in-memory tail (events not necessarily flushed to the DB yet).
    // The client also fetches /events for durable history and dedupes by seq.
    for env in snapshot {
        if send_envelope(&mut socket, &env).await.is_err() {
            hub.drop_if_idle(task_id).await;
            return;
        }
    }

    loop {
        tokio::select! {
            // Outbound: a live frame from the hub.
            ev = rx.recv() => match ev {
                Ok(env) => {
                    if send_envelope(&mut socket, &env).await.is_err() {
                        break;
                    }
                }
                // Slow consumer fell behind — tell the client to refetch history.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                // Channel closed: the session ended. Done streaming.
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            // Inbound: an operator message, or the socket closing.
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(text))) => handle_inbound(&hub, task_id, &text).await,
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {} // ignore ping/pong/binary
                Some(Err(_)) => break,
            },
        }
    }

    hub.drop_if_idle(task_id).await;
}

async fn send_envelope(socket: &mut WebSocket, env: &crate::jobs::hub::Envelope) -> Result<(), ()> {
    let json = serde_json::to_string(env).map_err(|_| ())?;
    socket.send(Message::Text(json.into())).await.map_err(|_| ())
}

async fn handle_inbound(hub: &crate::jobs::hub::LiveSessions, task_id: Uuid, text: &str) {
    let Ok(msg) = serde_json::from_str::<Inbound>(text) else { return };
    match msg {
        Inbound::Chat { text } => {
            hub.send_to_agent(task_id, &text).await;
        }
        Inbound::Redefine { text } => {
            hub.send_to_agent(task_id, &format!("New goal: {text}")).await;
        }
        Inbound::Stop => {
            hub.stop(task_id).await;
        }
    }
}
