//! Task-state lifecycle: the two orthogonal axes (durable `agent_state` +
//! operator-owned `task_state`), the read-time derivation that overlays the
//! live hub's warm/running disposition, orphan recovery, and the small
//! DB-write helpers the runner uses to advance a task through its life.
//!
//! Split out of `store.rs` (over the 400-line cap) along the lifecycle seam.

use anyhow::{Context, Result};
use chrono::Utc;
use sea_orm::*;
use tracing::{error, info};
use uuid::Uuid;

use crate::entity::{task_results, tasks};
use crate::jobs::hub::LiveSessions;
use crate::jobs::store::TaskStore;

/// Durable `agent_state` values (the only ones ever persisted). `warm`/`running`
/// are derived at read time and never written.
pub const AGENT_COLD: &str = "cold";
pub const AGENT_PENDING: &str = "pending";
pub const AGENT_FAILED: &str = "failed";

/// `task_state` values (the operator lifecycle).
pub const TASK_PENDING: &str = "pending";
pub const TASK_WORKING_ON: &str = "working_on";
pub const TASK_COMPLETED: &str = "completed";
pub const TASK_FAILED: &str = "failed";

/// The four valid `task_state` values an operator may PATCH a task to.
pub const TASK_STATES: [&str; 4] = [TASK_PENDING, TASK_WORKING_ON, TASK_COMPLETED, TASK_FAILED];

/// Read-time `agent_state`: the live hub's volatile disposition wins, falling
/// back to the durable column. `running` (a turn is processing) beats `warm`
/// (an idle live agent) beats the persisted backing.
pub fn derive_agent_state(durable: &str, id: Uuid, hub: &LiveSessions) -> &'static str {
    if hub.is_running(id) {
        return "running";
    }
    if hub.is_warm_sync(id) {
        return "warm";
    }
    match durable {
        AGENT_PENDING => "pending",
        AGENT_FAILED => "failed",
        _ => "cold",
    }
}

/// Map an old (pre-split) `status` value to the new `(task_state, agent_state)`
/// pair — the canonical reference for the m20260608 migration's backfill SQL,
/// kept here so that mapping is unit-testable independently of a live database.
#[cfg_attr(not(test), allow(dead_code))]
pub fn migrate_status(old: &str) -> (&'static str, &'static str) {
    match old {
        "pending" => (TASK_PENDING, AGENT_COLD),
        "running" => (TASK_WORKING_ON, AGENT_COLD),
        "completed" => (TASK_COMPLETED, AGENT_COLD),
        "failed" => (TASK_FAILED, AGENT_FAILED),
        "killed" => (TASK_FAILED, AGENT_FAILED),
        _ => (TASK_FAILED, AGENT_FAILED),
    }
}

impl TaskStore {
    /// At startup, reconcile tasks that were mid-flight when the previous process
    /// died. Durable `agent_state` only ever persists cold|pending|failed, and the
    /// warm/running disposition lives in-memory (lost on restart), so a task that
    /// looks live can only be an orphan. Two unfinished cases are reconciled:
    /// `task_state = working_on` (was actively running) → failed, with the reason
    /// noted in the result text; and durable `agent_state = pending` (confirmed but
    /// crashed before its first turn, spawn intent lost) → rewound to `cold` so the
    /// operator can Run it again.
    ///
    /// Never-confirmed pending tasks carry durable `cold`, so they're untouched and
    /// stay legitimately pending. (The pre-split behavior also killed pending
    /// tasks, but killing un-started work on every restart is harsh.)
    pub async fn recover_orphans(&self) -> Result<u64> {
        // Warm/running are in-memory only, so on restart the hub is empty and any
        // task that was mid-flight is now an orphan. Two cases, both with a null
        // finished_at: a task that was actively working (`task_state=working_on`)
        // is dead work → mark failed; a task whose durable `agent_state` is still
        // `pending` was confirmed but crashed before its first turn (the spawn
        // intent is lost) → reset it to `cold` so the operator can Run it again.
        // Genuinely-pending, never-confirmed tasks carry durable `cold`, so they
        // are untouched.
        let orphans = tasks::Entity::find()
            .filter(tasks::Column::FinishedAt.is_null())
            .filter(
                Condition::any()
                    .add(tasks::Column::TaskState.eq(TASK_WORKING_ON))
                    .add(tasks::Column::AgentState.eq(AGENT_PENDING)),
            )
            .all(self.db())
            .await
            .context("query orphan tasks")?;

        let count = orphans.len() as u64;
        if count == 0 {
            return Ok(0);
        }

        for t in orphans {
            let id = t.id;
            let was_working = t.task_state == TASK_WORKING_ON;
            let mut active: tasks::ActiveModel = t.into();
            active.pid = Set(None);
            if was_working {
                active.agent_state = Set(AGENT_FAILED.to_string());
                active.task_state = Set(TASK_FAILED.to_string());
                active.finished_at = Set(Some(Utc::now().into()));
            } else {
                // Confirmed but never started: rewind the durable backing to cold
                // (task_state stays pending → the SPA shows Run again).
                active.agent_state = Set(AGENT_COLD.to_string());
            }
            if let Err(e) = active.update(self.db()).await {
                error!(%id, error = %e, "failed to recover orphan task");
                continue;
            }
            if was_working {
                let _ = self.note_task_result(id, "killed: orphan on restart").await;
                info!(%id, "recovered orphan task → failed");
            } else {
                info!(%id, "recovered confirmed-but-unstarted task → cold");
            }
        }
        Ok(count)
    }

    /// Write the durable `agent_state` and `task_state` columns without touching
    /// `finished_at`. Used for the per-turn transitions of a warm session (which
    /// can still resume) and for confirm/pre-spawn. `started_at` is stamped once,
    /// the first time the task moves into `working_on`.
    pub(crate) async fn set_states(
        &self,
        task_id: Uuid,
        agent_state: &str,
        task_state: &str,
    ) -> Result<()> {
        let model = tasks::Entity::find_by_id(task_id)
            .one(self.db())
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        let stamp_start = task_state == TASK_WORKING_ON && model.started_at.is_none();
        let mut task: tasks::ActiveModel = model.into();
        task.agent_state = Set(agent_state.to_string());
        task.task_state = Set(task_state.to_string());
        if stamp_start {
            task.started_at = Set(Some(Utc::now().into()));
        }
        task.update(self.db()).await?;
        self.publish_state(task_id, agent_state, task_state).await;
        Ok(())
    }

    /// Terminal finish: set both state columns and stamp `finished_at`. Used for
    /// graceful session end, budget kill, non-zero exit, and force-delete
    /// fallback. An optional note is appended to the task's result text.
    pub(crate) async fn finish_task(
        &self,
        task_id: Uuid,
        agent_state: &str,
        task_state: &str,
        note: Option<&str>,
    ) -> Result<()> {
        let mut task: tasks::ActiveModel = tasks::Entity::find_by_id(task_id)
            .one(self.db())
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?
            .into();
        task.agent_state = Set(agent_state.to_string());
        task.task_state = Set(task_state.to_string());
        task.finished_at = Set(Some(Utc::now().into()));
        task.update(self.db()).await?;
        if let Some(note) = note {
            let _ = self.note_task_result(task_id, note).await;
        }
        self.publish_state(task_id, agent_state, task_state).await;
        Ok(())
    }

    /// Record a short disposition note (paused / budget-killed / orphan) as the
    /// task's result text. If a result row already exists, the note is appended;
    /// otherwise a minimal row is created. The disposition reason no longer lives
    /// in a `status` value, so it has to be recorded here.
    pub(crate) async fn note_task_result(&self, task_id: Uuid, text: &str) -> Result<()> {
        let existing = task_results::Entity::find()
            .filter(task_results::Column::TaskId.eq(task_id))
            .one(self.db())
            .await
            .context("looking up task result for note")?;

        match existing {
            Some(row) => {
                let combined = if row.result_text.is_empty() {
                    text.to_string()
                } else {
                    format!("{}\n{text}", row.result_text)
                };
                let mut active: task_results::ActiveModel = row.into();
                active.result_text = Set(combined);
                active
                    .update(self.db())
                    .await
                    .context("appending result note")?;
            }
            None => {
                let row = task_results::ActiveModel {
                    id: Set(Uuid::new_v4()),
                    task_id: Set(task_id),
                    cost_usd: Set(0.0),
                    input_tokens: Set(0),
                    output_tokens: Set(0),
                    num_turns: Set(0),
                    is_error: Set(false),
                    result_text: Set(text.to_string()),
                    session_id: Set(String::new()),
                };
                task_results::Entity::insert(row)
                    .exec(self.db())
                    .await
                    .context("inserting result note")?;
            }
        }
        Ok(())
    }

    /// Push a state change to live WS subscribers so the SPA reflects the two
    /// axes without polling. Emits the durable backing AND the derived
    /// agent_state, mirroring the REST contract.
    async fn publish_state(&self, task_id: Uuid, durable_agent: &str, task_state: &str) {
        let agent_state = derive_agent_state(durable_agent, task_id, self.hub());
        self.hub()
            .publish_aux(
                task_id,
                crate::jobs::hub::EnvelopeKind::Status,
                serde_json::json!({ "agent_state": agent_state, "task_state": task_state }),
            )
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_agent_state_matrix() {
        let hub = LiveSessions::detached();
        let id = Uuid::new_v4();

        // No live channel: durable backing decides.
        assert_eq!(derive_agent_state("pending", id, &hub), "pending");
        assert_eq!(derive_agent_state("failed", id, &hub), "failed");
        assert_eq!(derive_agent_state("cold", id, &hub), "cold");
        // Any other durable value collapses to cold (the narrowed set never
        // persists anything else, but be defensive).
        assert_eq!(derive_agent_state("whatever", id, &hub), "cold");

        // Warm (idle live agent) overlays `warm` regardless of durable backing.
        hub.insert_test_channel(id, true, false);
        for durable in ["pending", "failed", "cold"] {
            assert_eq!(derive_agent_state(durable, id, &hub), "warm");
        }

        // Running (an active turn) wins over warm and over the durable column.
        hub.insert_test_channel(id, true, true);
        for durable in ["pending", "failed", "cold"] {
            assert_eq!(derive_agent_state(durable, id, &hub), "running");
        }

        // Running can be true even if the warm mirror lags — running still wins.
        hub.insert_test_channel(id, false, true);
        assert_eq!(derive_agent_state("cold", id, &hub), "running");
    }

    #[test]
    fn migrate_status_full_matrix() {
        assert_eq!(migrate_status("pending"), ("pending", "cold"));
        assert_eq!(migrate_status("running"), ("working_on", "cold"));
        assert_eq!(migrate_status("completed"), ("completed", "cold"));
        assert_eq!(migrate_status("failed"), ("failed", "failed"));
        assert_eq!(migrate_status("killed"), ("failed", "failed"));
        // Unknown legacy values fail-safe to failed/failed.
        assert_eq!(migrate_status("bogus"), ("failed", "failed"));
    }

    // ---- DB-backed lifecycle (gated on DATABASE_URL) ----

    use std::sync::Arc;

    use migration::MigratorTrait;
    use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Statement};

    use crate::auth::store::AuthStore;
    use crate::auth::waiter::AuthWaiter;
    use crate::config::Config;
    use crate::jobs::hub::LiveSessions;
    use crate::jobs::registry::RunningTasks;
    use crate::jobs::store::TaskStore;
    use crate::jobs::types::TriggerReason;
    use crate::project::{ProjectStore, ProviderKind};
    use crate::provider::ProviderRegistry;
    use crate::service::{NewService, ServiceStore};
    use crate::workspace::Workspace;

    async fn fresh_db() -> Option<(DatabaseConnection, String, String)> {
        let base = std::env::var("DATABASE_URL").ok()?;
        let slash = base.rfind('/')?;
        let admin_url = format!("{}/postgres", &base[..slash]);
        let db_name = format!("agent_lifecycle_{}", Uuid::new_v4().simple());
        let admin = Database::connect(&admin_url).await.ok()?;
        admin
            .execute(Statement::from_string(
                admin.get_database_backend(),
                format!("CREATE DATABASE \"{db_name}\""),
            ))
            .await
            .ok()?;
        let test_url = format!("{}/{db_name}", &base[..slash]);
        let conn = Database::connect(&test_url).await.ok()?;
        migration::Migrator::up(&conn, None).await.ok()?;
        Some((conn, db_name, admin_url))
    }

    async fn drop_db(admin_url: &str, db_name: &str) {
        if let Ok(admin) = Database::connect(admin_url).await {
            let _ = admin
                .execute(Statement::from_string(
                    admin.get_database_backend(),
                    format!("DROP DATABASE IF EXISTS \"{db_name}\" WITH (FORCE)"),
                ))
                .await;
        }
    }

    fn store_with(db: &DatabaseConnection, hub: LiveSessions) -> Arc<TaskStore> {
        let config = Config {
            api_bearer_token: None,
            database_url: String::new(),
            repo_base_path: "/tmp/agent-test".into(),
            max_concurrent_jobs: 3,
            listen_addr: String::new(),
            public_base_url: None,
            task_token_budget: 1_000_000,
        };
        Arc::new(TaskStore::new(
            db.clone(),
            config,
            ProviderRegistry::new(ServiceStore::new(db.clone())),
            Arc::new(ProjectStore::new(db.clone())),
            Arc::new(Workspace::new("/tmp/agent-test")),
            RunningTasks::new(),
            hub,
            Arc::new(AuthStore::new(db.clone())),
            AuthWaiter::new(),
        ))
    }

    /// Insert a service row so `tasks.service_id` satisfies its FK, and
    /// return its id.
    async fn seed_service(db: &DatabaseConnection) -> Uuid {
        ServiceStore::new(db.clone())
            .create(NewService {
                kind: ProviderKind::Github,
                slug: format!("svc-{}", Uuid::new_v4().simple()),
                display_name: "test".into(),
                base_url: "https://example.test".into(),
                token: "t".into(),
                webhook_secret: "s".into(),
                bot_username: "bot".into(),
                autofire: false,
                auth_kind: Default::default(),
                app_credentials: None,
                trigger_mode: Default::default(),
                trigger_label: String::new(),
            })
            .await
            .expect("seed service")
            .id
    }

    /// create → confirm(pre-spawn) → run → warm → PATCH → terminal finish,
    /// asserting both `task_state` (persisted) and the derived `agent_state` at
    /// each step. Replays the exact column writes + hub flags the runner makes,
    /// so the persistence + derivation are exercised without a live claude.
    #[tokio::test]
    async fn full_lifecycle_axes() {
        let Some((db, name, admin)) = fresh_db().await else {
            eprintln!("DATABASE_URL not set; skipping DB lifecycle test");
            return;
        };
        let hub = LiveSessions::new(db.clone());
        let store = store_with(&db, hub.clone());
        let service_id = seed_service(&db).await;

        let id = store
            .create_task(
                TriggerReason::Issue {
                    iid: 7,
                    title: "demo".into(),
                    description: String::new(),
                    url: "http://x/7".into(),
                },
                service_id,
                ProviderKind::Github,
                None,
                "acme/widgets".into(),
                "git@x:acme/widgets.git".into(),
                "main".into(),
            )
            .await
            .unwrap();

        // create → cold / pending → derives cold.
        let (t, _) = store.get_task(id).await.unwrap().unwrap();
        assert_eq!(
            (t.agent_state.as_str(), t.task_state.as_str()),
            ("cold", "pending")
        );
        assert_eq!(derive_agent_state(&t.agent_state, id, &hub), "cold");

        // confirm pre-spawn: durable agent_state → pending.
        store.set_states(id, "pending", "pending").await.unwrap();
        let (t, _) = store.get_task(id).await.unwrap().unwrap();
        assert_eq!(t.agent_state, "pending");
        assert_eq!(derive_agent_state(&t.agent_state, id, &hub), "pending");

        // turn start: live channel + running flag; durable stays cold,
        // task_state → working_on; started_at stamped.
        hub.insert_test_channel(id, true, true);
        store.set_states(id, "cold", "working_on").await.unwrap();
        let (t, _) = store.get_task(id).await.unwrap().unwrap();
        assert_eq!(
            (t.agent_state.as_str(), t.task_state.as_str()),
            ("cold", "working_on")
        );
        assert!(t.started_at.is_some());
        assert_eq!(derive_agent_state(&t.agent_state, id, &hub), "running");

        // warm idle: running flag clears, stdin still attached; durable cold,
        // task_state → completed but NO finished_at (it can resume).
        hub.insert_test_channel(id, true, false);
        store.set_states(id, "cold", "completed").await.unwrap();
        let (t, _) = store.get_task(id).await.unwrap().unwrap();
        assert_eq!(
            (t.agent_state.as_str(), t.task_state.as_str()),
            ("cold", "completed")
        );
        assert!(t.finished_at.is_none());
        assert_eq!(derive_agent_state(&t.agent_state, id, &hub), "warm");

        // operator PATCH back to working_on (allowed on any state); derived stays
        // warm (agent still attached), durable untouched.
        store
            .update_task(
                id,
                crate::jobs::store::TaskEdits {
                    branch: None,
                    default_branch: None,
                    task_state: Some("working_on".into()),
                },
            )
            .await
            .unwrap();
        let (t, _) = store.get_task(id).await.unwrap().unwrap();
        assert_eq!(t.task_state, "working_on");
        assert_eq!(t.agent_state, "cold");
        assert_eq!(derive_agent_state(&t.agent_state, id, &hub), "warm");

        // session ends: drop the channel, terminal finish stamps finished_at.
        hub.end(id).await;
        store
            .finish_task(id, "cold", "completed", None)
            .await
            .unwrap();
        let (t, _) = store.get_task(id).await.unwrap().unwrap();
        assert_eq!(t.task_state, "completed");
        assert!(t.finished_at.is_some());
        assert_eq!(derive_agent_state(&t.agent_state, id, &hub), "cold");

        // budget kill path records a note + failed/failed.
        store
            .finish_task(id, "failed", "failed", Some("killed: token budget"))
            .await
            .unwrap();
        let (_, result) = store.get_task(id).await.unwrap().unwrap();
        assert!(result.unwrap().result_text.contains("token budget"));

        drop(store);
        drop(db);
        drop_db(&admin, &name).await;
    }

    /// recover_orphans flips an in-flight (working_on, no finished_at) task to
    /// failed and notes it, rewinds a confirmed-but-unstarted task (durable
    /// `pending`) back to `cold` so it can Run again, and leaves a never-confirmed
    /// pending task (durable `cold`) untouched.
    #[tokio::test]
    async fn recover_orphans_reconciles_inflight_only() {
        let Some((db, name, admin)) = fresh_db().await else {
            return;
        };
        let hub = LiveSessions::new(db.clone());
        let store = store_with(&db, hub);
        let svc = seed_service(&db).await;

        let mk = |iid: u64| TriggerReason::Issue {
            iid,
            title: format!("t{iid}"),
            description: String::new(),
            url: format!("http://x/{iid}"),
        };
        let inflight = store
            .create_task(
                mk(1),
                svc,
                ProviderKind::Github,
                None,
                "p".into(),
                "git@x:p.git".into(),
                "main".into(),
            )
            .await
            .unwrap();
        let confirmed = store
            .create_task(
                mk(2),
                svc,
                ProviderKind::Github,
                None,
                "p".into(),
                "git@x:p.git".into(),
                "main".into(),
            )
            .await
            .unwrap();
        let pending = store
            .create_task(
                mk(3),
                svc,
                ProviderKind::Github,
                None,
                "p".into(),
                "git@x:p.git".into(),
                "main".into(),
            )
            .await
            .unwrap();
        // Simulate a turn in flight (no finished_at).
        store
            .set_states(inflight, "cold", "working_on")
            .await
            .unwrap();
        // Simulate a confirmed-but-unstarted task (durable pending, crashed before
        // its first turn).
        store
            .set_states(confirmed, "pending", "pending")
            .await
            .unwrap();

        let n = store.recover_orphans().await.unwrap();
        assert_eq!(n, 2, "in-flight + confirmed-unstarted are orphans");

        let (t, result) = store.get_task(inflight).await.unwrap().unwrap();
        assert_eq!(
            (t.agent_state.as_str(), t.task_state.as_str()),
            ("failed", "failed")
        );
        assert!(t.finished_at.is_some());
        assert!(result.unwrap().result_text.contains("orphan"));

        // Confirmed-but-unstarted → rewound to cold/pending (Run shows again), no
        // finished_at, no orphan note.
        let (c, c_result) = store.get_task(confirmed).await.unwrap().unwrap();
        assert_eq!(
            (c.agent_state.as_str(), c.task_state.as_str()),
            ("cold", "pending")
        );
        assert!(c.finished_at.is_none());
        assert!(c_result.is_none());

        let (p, _) = store.get_task(pending).await.unwrap().unwrap();
        assert_eq!(
            (p.agent_state.as_str(), p.task_state.as_str()),
            ("cold", "pending")
        );

        drop(store);
        drop(db);
        drop_db(&admin, &name).await;
    }
}
