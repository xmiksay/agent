use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Notify;
use uuid::Uuid;

/// Lets one task await an external resolution event keyed by auth request id.
#[derive(Clone, Default)]
pub struct AuthWaiter {
    notifiers: Arc<DashMap<Uuid, Arc<Notify>>>,
}

impl AuthWaiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a notifier for `id`. Returns the same Arc on subsequent calls.
    pub fn register(&self, id: Uuid) -> Arc<Notify> {
        self.notifiers
            .entry(id)
            .or_insert_with(|| Arc::new(Notify::new()))
            .clone()
    }

    /// Wake everyone waiting on `id` and drop the entry.
    pub fn notify(&self, id: Uuid) {
        if let Some((_, n)) = self.notifiers.remove(&id) {
            n.notify_waiters();
        }
    }

    pub fn pending_ids(&self) -> Vec<Uuid> {
        self.notifiers.iter().map(|e| *e.key()).collect()
    }

    /// Convenience: number of in-flight waits — useful in tests/observability.
    pub fn inflight(&self) -> HashMap<Uuid, usize> {
        self.notifiers
            .iter()
            .map(|e| (*e.key(), Arc::strong_count(e.value())))
            .collect()
    }
}
