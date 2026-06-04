//! Live task registry. Maps task_id → AbortHandle so the operator can kill
//! a running job; the runner sets `kill_on_drop(true)` on the claude child,
//! so aborting the spawn drops the Child and SIGKILLs it.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct RunningTasks {
    inner: Arc<Mutex<HashMap<Uuid, AbortHandle>>>,
}

impl RunningTasks {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, task_id: Uuid, handle: AbortHandle) {
        self.inner.lock().await.insert(task_id, handle);
    }

    pub async fn unregister(&self, task_id: Uuid) {
        self.inner.lock().await.remove(&task_id);
    }

    /// Returns true if a running task was found and aborted.
    pub async fn abort(&self, task_id: Uuid) -> bool {
        let mut guard = self.inner.lock().await;
        if let Some(handle) = guard.remove(&task_id) {
            handle.abort();
            true
        } else {
            false
        }
    }
}
