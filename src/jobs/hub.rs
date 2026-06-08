//! Live session hub: per-task fan-out of agent events to WebSocket clients,
//! durable batching into `tasks.event_log`, and the back-channels that write
//! operator messages and control responses to the running agent's stdin.
//!
//! One `TaskChannel` exists per *watched or active* task. The runner calls
//! [`LiveSessions::register`] at session start (attaching the stdin senders and
//! seeding the sequence number from the persisted history length) and
//! [`LiveSessions::end`] at exit. A WebSocket handler calls
//! [`LiveSessions::subscribe`], which lazily creates the channel if the session
//! hasn't registered yet — so a socket opened during a Resume race still attaches
//! to the same channel the runner fills moments later.
//!
//! Frames keep a single monotonic `seq` per task. The in-memory `history` holds
//! the whole active session; `flushed` tracks how much of it is already in the
//! DB. Every frame kind (agent event, auth_request, status) consumes a `seq` and
//! is persisted to `task_events` with its `kind`, so the persisted history is a
//! complete, contiguous (seqs 0..N-1) record of both directions of the session.
//! The seq counter is seeded from the persisted length, so the frontend can
//! dedupe REST history against live frames by `seq` with no gaps.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use dashmap::DashMap;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::{error, warn};
use uuid::Uuid;

use crate::agent::{AgentBackend, PermissionDecision};
use crate::entity::task_events;

/// Persist to `event_log` once this many unflushed events accumulate.
const FLUSH_BATCH: usize = 100;
/// Backlog for the process-wide stream that carries every task's frames to the
/// single global WebSocket. It multiplexes all active tasks; a client that lags
/// past this skips frames and refetches history via REST.
const ALL_BROADCAST_CAP: usize = 4096;

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

impl EnvelopeKind {
    /// Stored in the `task_events.kind` column; matches the serde snake_case
    /// wire form so REST history and live frames share one vocabulary.
    fn as_db_str(&self) -> &'static str {
        match self {
            EnvelopeKind::Event => "event",
            EnvelopeKind::AuthRequest => "auth_request",
            EnvelopeKind::Status => "status",
        }
    }
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
    /// `(seq, kind, payload)` for the whole active session; `flushed` counts how
    /// many leading entries are already persisted in `task_events`.
    items: Vec<(u64, EnvelopeKind, Value)>,
    flushed: usize,
}

struct TaskChannel {
    /// The backend driving this session — owns event naming and the
    /// stdin-encoding of operator messages so the WS layer stays agent-agnostic.
    backend: Arc<dyn AgentBackend>,
    next_seq: AtomicU64,
    /// Whether a turn is actively processing (between turn-start and the
    /// permit release). Volatile, in-memory only — overlaid onto the derived
    /// `agent_state` so a warm-but-idle agent reads as `warm`, not `running`.
    running: AtomicBool,
    /// Mirrors "an stdin sender is attached" for lock-free reads. The authoritative
    /// presence lives behind `stdin` (a Mutex); this flag tracks it so the
    /// read-time `derive_agent_state` can stay synchronous.
    warm: AtomicBool,
    history: Mutex<History>,
    /// Operator-message channel — present only while a runner session is live.
    /// The runner's turn loop drains this one message per turn for pacing.
    stdin: Mutex<Option<mpsc::Sender<String>>>,
    /// Raw-line channel to the agent's stdin writer task. Carries control
    /// responses, which must reach stdin immediately (mid-turn), bypassing the
    /// per-turn pacing of `stdin`. Present only while a session is live.
    control: Mutex<Option<mpsc::Sender<String>>>,
}

impl TaskChannel {
    fn new(backend: Arc<dyn AgentBackend>, start_seq: u64) -> Self {
        Self {
            backend,
            next_seq: AtomicU64::new(start_seq),
            running: AtomicBool::new(false),
            warm: AtomicBool::new(false),
            history: Mutex::new(History {
                items: Vec::new(),
                flushed: 0,
            }),
            stdin: Mutex::new(None),
            control: Mutex::new(None),
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
    /// Process-wide fan-out of every task's frames. One global WebSocket
    /// subscribes here and routes by `task_id`, so the browser holds a single
    /// connection instead of one per task.
    all: broadcast::Sender<Envelope>,
}

impl LiveSessions {
    pub fn new(db: DatabaseConnection) -> Self {
        let (all, _) = broadcast::channel(ALL_BROADCAST_CAP);
        Self {
            db,
            channels: Arc::new(DashMap::new()),
            all,
        }
    }

    /// Subscribe to the process-wide stream of all tasks' frames.
    pub fn subscribe_all(&self) -> broadcast::Receiver<Envelope> {
        self.all.subscribe()
    }

    fn get(&self, task_id: Uuid) -> Option<Arc<TaskChannel>> {
        self.channels.get(&task_id).map(|c| c.clone())
    }

    /// Attach a live session: store the stdin senders and seed the sequence
    /// counter from the persisted event count so live `seq`s continue where the
    /// DB history left off. Creates the channel if a subscriber raced ahead.
    /// `stdin` carries operator messages (drained one per turn); `control`
    /// carries control responses straight to the stdin writer task.
    pub async fn register(
        &self,
        task_id: Uuid,
        backend: Arc<dyn AgentBackend>,
        stdin: mpsc::Sender<String>,
        control: mpsc::Sender<String>,
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
        *ch.control.lock().await = Some(control);
        ch.warm.store(true, Ordering::SeqCst);
    }

    /// Publish a raw agent event. See [`Self::publish`]. Called sequentially by a
    /// single stdout reader per session.
    pub async fn publish_event(&self, task_id: Uuid, value: Value) {
        self.publish(task_id, EnvelopeKind::Event, value).await;
    }

    /// Publish a side-channel frame (approval / status). Like `publish_event`, it
    /// now consumes a `seq` and persists — every frame is durable history.
    pub async fn publish_aux(&self, task_id: Uuid, kind: EnvelopeKind, payload: Value) {
        self.publish(task_id, kind, payload).await;
    }

    /// Assign a seq, broadcast the envelope, append to history, and flush a batch
    /// to the DB once `FLUSH_BATCH` frames are pending. No-op if nobody is
    /// watching this task.
    ///
    /// May run concurrently (stdout reader, permission handler, HTTP resolve, and
    /// the lifecycle state writers all publish): `fetch_add` keeps seqs unique and the history
    /// `Mutex` serializes appends. Broadcast order may differ slightly from seq
    /// order, but consumers key by `seq`.
    async fn publish(&self, task_id: Uuid, kind: EnvelopeKind, value: Value) {
        let Some(ch) = self.get(task_id) else { return };
        let seq = ch.next_seq.fetch_add(1, Ordering::SeqCst);
        let _ = self.all.send(Envelope {
            task_id,
            agent: ch.agent(),
            seq,
            kind,
            payload: value.clone(),
        });
        let batch = {
            let mut h = ch.history.lock().await;
            h.items.push((seq, kind, value));
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

    /// Encode an operator message in the backend's stdin format and write it to
    /// the running agent. Returns false if no live session is attached.
    pub async fn send_to_agent(&self, task_id: Uuid, text: &str) -> bool {
        let Some(ch) = self.get(task_id) else {
            return false;
        };
        let line = ch.backend.encode_user_message(text);
        let stdin = ch.stdin.lock().await;
        match stdin.as_ref() {
            Some(tx) => tx.send(line).await.is_ok(),
            None => false,
        }
    }

    /// Encode a permission decision and send it straight to the agent's stdin
    /// writer, bypassing per-turn pacing so a mid-turn `can_use_tool` is
    /// answered without waiting for the turn to end. False if no live session.
    pub async fn respond_permission(
        &self,
        task_id: Uuid,
        request_id: &str,
        decision: PermissionDecision,
    ) -> bool {
        let Some(ch) = self.get(task_id) else {
            return false;
        };
        let line = ch.backend.encode_permission_response(request_id, &decision);
        let control = ch.control.lock().await;
        match control.as_ref() {
            Some(tx) => tx.send(line).await.is_ok(),
            None => false,
        }
    }

    /// Whether a live (warm) session is attached — i.e. an agent process is alive
    /// and accepting messages on stdin, even if the task is idle between turns.
    pub async fn is_warm(&self, task_id: Uuid) -> bool {
        match self.get(task_id) {
            Some(ch) => ch.stdin.lock().await.is_some(),
            None => false,
        }
    }

    /// Lock-free counterpart to `is_warm`, backed by the `warm` mirror flag.
    /// Lets the synchronous `derive_agent_state` read warmth without awaiting.
    pub fn is_warm_sync(&self, task_id: Uuid) -> bool {
        match self.get(task_id) {
            Some(ch) => ch.warm.load(Ordering::SeqCst),
            None => false,
        }
    }

    /// Whether a turn is actively processing right now (channel exists AND its
    /// `running` flag is set). Distinct from `is_warm`, which is true for an
    /// idle-between-turns agent too.
    pub fn is_running(&self, task_id: Uuid) -> bool {
        match self.get(task_id) {
            Some(ch) => ch.running.load(Ordering::SeqCst),
            None => false,
        }
    }

    /// Mark the task's turn as actively running. No-op if no channel is live.
    pub fn mark_running(&self, task_id: Uuid) {
        if let Some(ch) = self.get(task_id) {
            ch.running.store(true, Ordering::SeqCst);
        }
    }

    /// Clear the active-turn flag (agent goes idle/warm). No-op if no channel.
    pub fn mark_idle(&self, task_id: Uuid) {
        if let Some(ch) = self.get(task_id) {
            ch.running.store(false, Ordering::SeqCst);
        }
    }

    /// Graceful stop: drop the stdin sender so the agent's ChildStdin hits EOF
    /// and the process exits after finishing the current turn.
    pub async fn stop(&self, task_id: Uuid) -> bool {
        let Some(ch) = self.get(task_id) else {
            return false;
        };
        ch.warm.store(false, Ordering::SeqCst);
        ch.stdin.lock().await.take().is_some()
    }

    /// End a session: flush the unpersisted tail and drop the channel (closing
    /// subscriber receivers, which makes their sockets finish).
    pub async fn end(&self, task_id: Uuid) {
        if let Some(ch) = self.get(task_id) {
            ch.warm.store(false, Ordering::SeqCst);
            ch.running.store(false, Ordering::SeqCst);
            // Drop both stdin senders so the agent's writer task sees its channel
            // close (EOF on child stdin) even if a transient `get()` clone of the
            // Arc briefly outlives the map removal below.
            ch.stdin.lock().await.take();
            ch.control.lock().await.take();
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

    /// Persisted frame count for a task — the seq seed for a (re)starting
    /// session. Equals the next seq because every frame is persisted
    /// contiguously, so live `seq`s continue past the durable history.
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

    /// Append a batch of frames to `task_events` (one row per frame). Best-effort:
    /// a failed flush is logged, and the frames remain available live — history
    /// just isn't durable for those.
    async fn append_to_db(&self, task_id: Uuid, batch: &[(u64, EnvelopeKind, Value)]) {
        if batch.is_empty() {
            return;
        }
        let rows: Vec<task_events::ActiveModel> = batch
            .iter()
            .map(|(seq, kind, payload)| task_events::ActiveModel {
                task_id: Set(task_id),
                seq: Set(*seq as i64),
                kind: Set(kind.as_db_str().to_string()),
                payload: Set(payload.clone()),
            })
            .collect();
        if let Err(e) = task_events::Entity::insert_many(rows).exec(&self.db).await {
            error!(%task_id, error = %e, "failed to flush event batch to task_events");
        }
    }
}

#[cfg(test)]
impl LiveSessions {
    /// Detached hub for unit tests: no DB I/O is exercised by the lock-free
    /// disposition methods (`is_running`/`is_warm_sync`/`mark_*`), so a
    /// disconnected connection is fine. Use `insert_test_channel` to seed a
    /// channel whose flags the test then toggles.
    pub fn detached() -> Self {
        let (all, _) = broadcast::channel(ALL_BROADCAST_CAP);
        Self {
            db: DatabaseConnection::Disconnected,
            channels: Arc::new(DashMap::new()),
            all,
        }
    }

    /// Seed a bare channel for `task_id` (no live stdin) so warm/running flags
    /// can be flipped in tests. `warm` mirrors an attached stdin sender.
    pub fn insert_test_channel(&self, task_id: Uuid, warm: bool, running: bool) {
        let ch = Arc::new(TaskChannel::new(Arc::new(crate::agent::ClaudeCode), 0));
        ch.warm.store(warm, Ordering::SeqCst);
        ch.running.store(running, Ordering::SeqCst);
        self.channels.insert(task_id, ch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_str_matches_serde_wire_form() {
        for kind in [
            EnvelopeKind::Event,
            EnvelopeKind::AuthRequest,
            EnvelopeKind::Status,
        ] {
            let wire = serde_json::to_value(kind).unwrap();
            assert_eq!(Value::String(kind.as_db_str().to_string()), wire);
        }
        assert_eq!(EnvelopeKind::Event.as_db_str(), "event");
        assert_eq!(EnvelopeKind::AuthRequest.as_db_str(), "auth_request");
        assert_eq!(EnvelopeKind::Status.as_db_str(), "status");
    }

    /// The flush trigger fires exactly when `FLUSH_BATCH` unflushed frames have
    /// accumulated — mirrors the threshold check in `publish`.
    #[test]
    fn batching_threshold_triggers_at_flush_batch() {
        let mut h = History {
            items: Vec::new(),
            flushed: 0,
        };
        for i in 0..FLUSH_BATCH - 1 {
            h.items.push((i as u64, EnvelopeKind::Event, Value::Null));
            assert!(h.items.len() - h.flushed < FLUSH_BATCH);
        }
        h.items
            .push((FLUSH_BATCH as u64 - 1, EnvelopeKind::Status, Value::Null));
        assert_eq!(h.items.len() - h.flushed, FLUSH_BATCH);
    }
}
