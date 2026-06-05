//! Live session hub: per-task fan-out of agent events to WebSocket clients,
//! durable batching into `tasks.event_log`, and the back-channel that writes
//! operator messages to the running agent's stdin.
//!
//! One `TaskChannel` exists per *watched or active* task. The runner calls
//! [`LiveSessions::register`] at session start (attaching the stdin sender and
//! seeding the sequence number from the persisted history length) and
//! [`LiveSessions::end`] at exit. A WebSocket handler calls
//! [`LiveSessions::subscribe`], which lazily creates the channel if the session
//! hasn't registered yet — so a socket opened during a Resume race still attaches
//! to the same channel the runner fills moments later.
//!
//! Events keep a single monotonic `seq` per task. The in-memory `history` holds
//! the whole active session; `flushed` tracks how much of it is already in the
//! DB. The persisted array index equals `seq` (the seq counter is seeded from the
//! persisted length), so the frontend can dedupe REST history against live frames
//! by `seq` with no gaps. Only `Event` frames consume a `seq` and are persisted;
//! `auth_request` / `status` frames are UI side-channels and do neither.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, Set,
};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{error, warn};
use uuid::Uuid;

use crate::agent::{AgentBackend, ClaudeCode};
use crate::entity::task_events;

/// Persist to `event_log` once this many unflushed events accumulate.
const FLUSH_BATCH: usize = 100;
/// Broadcast backlog per task. A client that lags past this gets a `Lagged`
/// error and reconnects (refetching history), so a small bound is fine.
const BROADCAST_CAP: usize = 1024;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeKind {
    /// A raw agent stream event (parsed and rendered on the frontend).
    Event,
    /// An auth_request row created/resolved for this task.
    AuthRequest,
    /// A task status change.
    Status,
}

/// One frame sent to WebSocket subscribers.
#[derive(Clone, Serialize)]
pub struct Envelope {
    pub task_id: Uuid,
    pub agent: String,
    pub seq: u64,
    pub kind: EnvelopeKind,
    pub payload: Value,
}

struct History {
    /// `(seq, payload)` for the whole active session; `flushed` counts how many
    /// leading entries are already persisted in `task_events`.
    items: Vec<(u64, Value)>,
    flushed: usize,
}

struct TaskChannel {
    /// The backend driving this session — owns event naming and the
    /// stdin-encoding of operator messages so the WS layer stays agent-agnostic.
    backend: Arc<dyn AgentBackend>,
    next_seq: AtomicU64,
    events: broadcast::Sender<Envelope>,
    history: Mutex<History>,
    /// Present only while a runner session is live; writes go to the agent stdin.
    stdin: Mutex<Option<mpsc::Sender<String>>>,
}

impl TaskChannel {
    fn new(backend: Arc<dyn AgentBackend>, start_seq: u64) -> Self {
        let (events, _) = broadcast::channel(BROADCAST_CAP);
        Self {
            backend,
            next_seq: AtomicU64::new(start_seq),
            events,
            history: Mutex::new(History { items: Vec::new(), flushed: 0 }),
            stdin: Mutex::new(None),
        }
    }

    fn agent(&self) -> String {
        self.backend.name().to_string()
    }
}

#[derive(Clone)]
pub struct LiveSessions {
    db: DatabaseConnection,
    channels: Arc<DashMap<Uuid, Arc<TaskChannel>>>,
}

impl LiveSessions {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db, channels: Arc::new(DashMap::new()) }
    }

    fn get(&self, task_id: Uuid) -> Option<Arc<TaskChannel>> {
        self.channels.get(&task_id).map(|c| c.clone())
    }

    /// Attach a live session: store the stdin sender and seed the sequence
    /// counter from the persisted event count so live `seq`s continue where the
    /// DB history left off. Creates the channel if a subscriber raced ahead.
    pub async fn register(
        &self,
        task_id: Uuid,
        backend: Arc<dyn AgentBackend>,
        stdin: mpsc::Sender<String>,
    ) {
        let start_seq = self.persisted_len(task_id).await;
        // Reuse a channel a subscriber may have created (preserving its live
        // receiver); otherwise create one with this backend. Then seed the seq
        // and attach stdin — no events flow until this point, so seeding is safe.
        let ch = self
            .channels
            .entry(task_id)
            .or_insert_with(|| Arc::new(TaskChannel::new(backend, start_seq)))
            .clone();
        ch.next_seq.store(start_seq, Ordering::SeqCst);
        *ch.stdin.lock().await = Some(stdin);
    }

    /// Publish a raw agent event: assign a seq, broadcast, capture `result`
    /// events for the end-of-session parse, append to history, and flush a batch
    /// to the DB once `FLUSH_BATCH` events are pending. Called sequentially by a
    /// single stdout reader per session.
    pub async fn publish_event(&self, task_id: Uuid, value: Value) {
        let Some(ch) = self.get(task_id) else { return };
        let seq = ch.next_seq.fetch_add(1, Ordering::SeqCst);
        let _ = ch.events.send(Envelope {
            task_id,
            agent: ch.agent(),
            seq,
            kind: EnvelopeKind::Event,
            payload: value.clone(),
        });
        let batch = {
            let mut h = ch.history.lock().await;
            h.items.push((seq, value));
            if h.items.len() - h.flushed >= FLUSH_BATCH {
                let batch = h.items[h.flushed..].to_vec();
                h.flushed = h.items.len();
                Some(batch)
            } else {
                None
            }
        };
        if let Some(batch) = batch {
            self.append_to_db(task_id, &batch).await;
        }
    }

    /// Broadcast a non-event side-channel frame (approval / status). Does not
    /// consume a `seq` or persist — no-op if nobody is watching this task.
    pub async fn publish_aux(&self, task_id: Uuid, kind: EnvelopeKind, payload: Value) {
        let Some(ch) = self.get(task_id) else { return };
        let seq = ch.next_seq.load(Ordering::SeqCst);
        let _ = ch.events.send(Envelope {
            task_id,
            agent: ch.agent(),
            seq,
            kind,
            payload,
        });
    }

    /// Subscribe a WebSocket client: returns the in-memory snapshot (active
    /// session events not necessarily flushed yet) plus a live receiver. Lazily
    /// creates the channel so a socket opened before the runner registers still
    /// attaches to the channel the runner will fill.
    pub async fn subscribe(&self, task_id: Uuid) -> (Vec<Envelope>, broadcast::Receiver<Envelope>) {
        let ch = self
            .channels
            .entry(task_id)
            .or_insert_with(|| Arc::new(TaskChannel::new(Arc::new(ClaudeCode), 0)))
            .clone();
        let rx = ch.events.subscribe();
        let h = ch.history.lock().await;
        let snapshot = h
            .items
            .iter()
            .map(|(seq, v)| Envelope {
                task_id,
                agent: ch.agent(),
                seq: *seq,
                kind: EnvelopeKind::Event,
                payload: v.clone(),
            })
            .collect();
        (snapshot, rx)
    }

    /// Encode an operator message in the backend's stdin format and write it to
    /// the running agent. Returns false if no live session is attached.
    pub async fn send_to_agent(&self, task_id: Uuid, text: &str) -> bool {
        let Some(ch) = self.get(task_id) else { return false };
        let line = ch.backend.encode_user_message(text);
        let stdin = ch.stdin.lock().await;
        match stdin.as_ref() {
            Some(tx) => tx.send(line).await.is_ok(),
            None => false,
        }
    }

    /// Graceful stop: drop the stdin sender so the agent's ChildStdin hits EOF
    /// and the process exits after finishing the current turn.
    pub async fn stop(&self, task_id: Uuid) -> bool {
        let Some(ch) = self.get(task_id) else { return false };
        ch.stdin.lock().await.take().is_some()
    }

    /// End a session: flush the unpersisted tail and drop the channel (closing
    /// subscriber receivers, which makes their sockets finish).
    pub async fn end(&self, task_id: Uuid) {
        if let Some(ch) = self.get(task_id) {
            let tail = {
                let mut h = ch.history.lock().await;
                let tail = h.items[h.flushed..].to_vec();
                h.flushed = h.items.len();
                tail
            };
            if !tail.is_empty() {
                self.append_to_db(task_id, &tail).await;
            }
        }
        self.channels.remove(&task_id);
    }

    /// Drop a channel a subscriber created for a task that never had a live
    /// session (best-effort; avoids leaking empty channels for inactive tasks).
    pub async fn drop_if_idle(&self, task_id: Uuid) {
        if let Some(ch) = self.get(task_id) {
            let no_session = ch.stdin.lock().await.is_none();
            if no_session && ch.events.receiver_count() == 0 {
                self.channels.remove(&task_id);
            }
        }
    }

    /// Persisted event count for a task — the seq seed for a (re)starting
    /// session, so live `seq`s continue past the durable history.
    async fn persisted_len(&self, task_id: Uuid) -> u64 {
        task_events::Entity::find()
            .filter(task_events::Column::TaskId.eq(task_id))
            .count(&self.db)
            .await
            .unwrap_or_else(|e| {
                warn!(%task_id, error = %e, "failed to count persisted task_events");
                0
            })
    }

    /// Append a batch of events to `task_events` (one row per event). Best-effort:
    /// a failed flush is logged, and the events remain available live — history
    /// just isn't durable for those.
    async fn append_to_db(&self, task_id: Uuid, batch: &[(u64, Value)]) {
        if batch.is_empty() {
            return;
        }
        let rows: Vec<task_events::ActiveModel> = batch
            .iter()
            .map(|(seq, payload)| task_events::ActiveModel {
                task_id: Set(task_id),
                seq: Set(*seq as i64),
                payload: Set(payload.clone()),
            })
            .collect();
        if let Err(e) = task_events::Entity::insert_many(rows).exec(&self.db).await {
            error!(%task_id, error = %e, "failed to flush event batch to task_events");
        }
    }
}
