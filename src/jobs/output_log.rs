//! In-memory ring buffer of recent task command output. Lost on restart.
//!
//! Entries are stored as `Arc<Mutex<TaskOutput>>` so the runner can append
//! stdout/stderr line-by-line while claude is still running, and HTTP readers
//! can pull a snapshot at any time.

use std::collections::VecDeque;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::Mutex;
use uuid::Uuid;

const MAX_ENTRIES: usize = 200;

#[derive(Clone, Debug, Serialize)]
pub struct TaskOutput {
    pub task_id: Uuid,
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub captured_at: DateTime<Utc>,
    /// false while the runner is still streaming; true once the child exits.
    pub finished: bool,
}

pub type LiveEntry = Arc<Mutex<TaskOutput>>;

#[derive(Clone, Default)]
pub struct TaskOutputLog {
    entries: Arc<Mutex<VecDeque<LiveEntry>>>,
}

impl TaskOutputLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserve a new entry and return a handle the runner can append to.
    /// Replaces any prior entry for the same task_id (e.g. retries).
    pub async fn start(&self, task_id: Uuid, command: String) -> LiveEntry {
        let entry = Arc::new(Mutex::new(TaskOutput {
            task_id,
            command,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            captured_at: Utc::now(),
            finished: false,
        }));
        let mut guard = self.entries.lock().await;
        // Drop any prior entry for the same task.
        let mut filtered: VecDeque<LiveEntry> = VecDeque::with_capacity(guard.len() + 1);
        for e in guard.drain(..) {
            let same = {
                let inner = e.lock().await;
                inner.task_id == task_id
            };
            if !same {
                filtered.push_back(e);
            }
        }
        *guard = filtered;
        if guard.len() >= MAX_ENTRIES {
            guard.pop_front();
        }
        guard.push_back(entry.clone());
        entry
    }

    /// Continue an existing entry in place — used by Resume. If no entry exists
    /// for `task_id`, behaves like `start`. Prior stdout/stderr are kept and
    /// a separator line is appended so the streamed view shows the join.
    pub async fn resume_or_start(&self, task_id: Uuid, command: String) -> LiveEntry {
        let guard = self.entries.lock().await;
        for entry in guard.iter() {
            let same = {
                let inner = entry.lock().await;
                inner.task_id == task_id
            };
            if same {
                let handle = entry.clone();
                drop(guard);
                {
                    let mut inner = handle.lock().await;
                    let sep = format!(
                        "\n{{\"type\":\"system\",\"subtype\":\"resume\",\"at\":\"{}\"}}\n",
                        Utc::now().to_rfc3339()
                    );
                    inner.stdout.push_str(&sep);
                    inner.command = command;
                    inner.exit_code = None;
                    inner.finished = false;
                    inner.captured_at = Utc::now();
                }
                return handle;
            }
        }
        drop(guard);
        self.start(task_id, command).await
    }

    pub async fn get(&self, task_id: Uuid) -> Option<TaskOutput> {
        let guard = self.entries.lock().await;
        for entry in guard.iter() {
            let inner = entry.lock().await;
            if inner.task_id == task_id {
                return Some(inner.clone());
            }
        }
        None
    }
}
