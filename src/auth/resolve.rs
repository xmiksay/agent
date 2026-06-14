//! Shared "resolve one approval and fan the result out" path. The single-resolve
//! route, bulk-resolve, and the finite-timeout auto-deny all need the identical
//! three steps — persist the decision, wake the parked permission handler, and
//! publish the resolution frame so the UI clears the pending card — so they live
//! here instead of being copied per call site.

use anyhow::Result;
use uuid::Uuid;

use crate::auth::store::{AuthRequest, AuthStatus, AuthStore};
use crate::auth::waiter::AuthWaiter;
use crate::jobs::hub::{EnvelopeKind, LiveSessions};

/// Resolve `id` to `decision`, notify the waiter, and publish the resolution to
/// the task's live hub. Returns the updated row.
pub async fn resolve_and_publish(
    auth_store: &AuthStore,
    auth_waiter: &AuthWaiter,
    hub: &LiveSessions,
    id: Uuid,
    decision: AuthStatus,
    reply: Option<String>,
) -> Result<AuthRequest> {
    let resolved = auth_store.resolve(id, decision, reply).await?;
    auth_waiter.notify(id);
    if let Ok(payload) = serde_json::to_value(&resolved) {
        hub.publish_aux(resolved.task_id, EnvelopeKind::AuthRequest, payload)
            .await;
    }
    Ok(resolved)
}
