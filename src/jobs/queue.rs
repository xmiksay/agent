//! Queue admission: the task-scheduling layer on top of `confirm_task`. A task
//! is **admitted** while its `run_job` future is alive (warm-idle, running, or
//! spawn-in-flight); free slots = `MAX_CONCURRENT_JOBS − running().len()`. This
//! is a distinct gate from the per-turn `Semaphore` in `store.rs` (which a warm
//! idle agent releases between turns) — here a warm agent still counts, because
//! its branch is reserved.
//!
//! Whenever a slot frees (a run future ends, an operator pause/delete, an
//! enqueue, or boot), `try_admit_next` pulls the highest-priority `pending`
//! queued task and confirms it. Split out of `store.rs`/`queries.rs` to keep both
//! under the 400-line cap.

use std::sync::Arc;

use anyhow::{Context, Result};
use sea_orm::*;
use tracing::{info, warn};
use uuid::Uuid;

use crate::entity::{queues, tasks};
use crate::jobs::lifecycle::{AGENT_COLD, TASK_PENDING};
use crate::jobs::store::TaskStore;

impl TaskStore {
    /// Is another *live* (warm or running) task occupying `branch` on this
    /// project? Returns that task's id, or `None`. `exclude` is the task being
    /// considered (never its own conflict). The "one agent per branch" guard,
    /// shared by `confirm_task` and the queue picker.
    pub(crate) async fn branch_is_live(
        &self,
        project_id: Uuid,
        branch: &str,
        exclude: Uuid,
    ) -> Result<Option<Uuid>> {
        let siblings = tasks::Entity::find()
            .filter(tasks::Column::ProjectId.eq(project_id))
            .filter(tasks::Column::Branch.eq(branch))
            .filter(tasks::Column::FinishedAt.is_null())
            .filter(tasks::Column::Id.ne(exclude))
            .all(self.db())
            .await
            .context("checking concurrent branch tasks")?;
        Ok(siblings
            .into_iter()
            .find(|t| self.hub().is_running(t.id) || self.hub().is_warm_sync(t.id))
            .map(|t| t.id))
    }

    /// The next queued task to admit: the highest-priority `pending`, `cold`,
    /// unfinished task that's enqueued (`queue_id` set) and whose branch isn't
    /// already live. Ordered by the owning queue's `priority`, then the task's
    /// in-queue `priority`, then age (oldest first). "Filter out completed" is the
    /// `task_state = pending` predicate.
    pub(crate) async fn pick_next_queued(&self) -> Result<Option<Uuid>> {
        let candidates = tasks::Entity::find()
            .filter(tasks::Column::QueueId.is_not_null())
            .filter(tasks::Column::TaskState.eq(TASK_PENDING))
            .filter(tasks::Column::AgentState.eq(AGENT_COLD))
            .filter(tasks::Column::FinishedAt.is_null())
            .join(JoinType::InnerJoin, tasks::Relation::Queue.def())
            .order_by_desc(queues::Column::Priority)
            .order_by_desc(tasks::Column::Priority)
            .order_by_asc(tasks::Column::CreatedAt)
            .all(self.db())
            .await
            .context("listing queued tasks")?;

        for task in candidates {
            // Skip a task whose branch already has a live agent — confirming it
            // would just bail. The next completion re-evaluates it.
            if let Some(branch) = task.branch.as_deref()
                && self
                    .branch_is_live(task.project_id, branch, task.id)
                    .await?
                    .is_some()
            {
                continue;
            }
            return Ok(Some(task.id));
        }
        Ok(None)
    }

    /// Pull queued tasks into open slots until the system is full or the queue is
    /// dry. Best-effort and idempotent: the `admit_lock` is held across the
    /// slot-count read and `confirm_task`, so two concurrent completions can't
    /// both claim the last slot. One completion can backfill several freed slots
    /// (e.g. after a bulk cancel), hence the loop.
    pub async fn try_admit_next(self: &Arc<Self>) {
        let _guard = self.admit_lock().lock().await;
        let max = self.max_concurrent_jobs();
        loop {
            if self.running().len().await >= max {
                return;
            }
            let next = match self.pick_next_queued().await {
                Ok(Some(id)) => id,
                Ok(None) => return,
                Err(e) => {
                    warn!(error = %e, "queue: picking next task failed");
                    return;
                }
            };
            if let Err(e) = self.confirm_task(next).await {
                // A losing race (branch became live, task changed) — log and
                // stop; the next completion re-evaluates the queue.
                warn!(%next, error = %e, "queue: admitting next task failed");
                return;
            }
            info!(%next, "queue: admitted task into a free slot");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;
    use migration::MigratorTrait;
    use sea_orm::*;
    use uuid::Uuid;

    use crate::auth::store::AuthStore;
    use crate::auth::waiter::AuthWaiter;
    use crate::config::Config;
    use crate::entity::{queues, tasks};
    use crate::jobs::hub::LiveSessions;
    use crate::jobs::lifecycle::{TASK_COMPLETED, TASK_PENDING};
    use crate::jobs::registry::RunningTasks;
    use crate::jobs::store::TaskStore;
    use crate::jobs::types::TriggerReason;
    use crate::project::{NewProjectConfig, ProjectStore, ProviderKind};
    use crate::provider::ProviderRegistry;
    use crate::service::{NewService, ServiceStore};
    use crate::workspace::Workspace;

    async fn fresh_db() -> Option<(DatabaseConnection, String, String)> {
        let base = std::env::var("DATABASE_URL").ok()?;
        let slash = base.rfind('/')?;
        let admin_url = format!("{}/postgres", &base[..slash]);
        let db_name = format!("agent_queue_{}", Uuid::new_v4().simple());
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

    fn store_with(db: &DatabaseConnection, hub: LiveSessions, max: usize) -> Arc<TaskStore> {
        let config = Config {
            api_bearer_token: None,
            database_url: String::new(),
            repo_base_path: "/tmp/agent-test".into(),
            max_concurrent_jobs: max,
            listen_addr: String::new(),
            public_base_url: None,
            task_token_budget: 1_000_000,
            operator_approval_timeout_secs: 0,
            job_timeout_secs: 0,
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

    async fn seed_project(db: &DatabaseConnection) -> Uuid {
        let service_id = ServiceStore::new(db.clone())
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
                models: None,
            })
            .await
            .expect("seed service")
            .id;
        ProjectStore::new(db.clone())
            .upsert_project(NewProjectConfig {
                provider: ProviderKind::Github,
                service_id,
                project_slug: format!("p-{}", Uuid::new_v4().simple()),
                full_name: "acme/widgets".into(),
                remote_url: "git@x:acme/widgets.git".into(),
                default_branch: "main".into(),
                my_username: "bot".into(),
            })
            .await
            .expect("seed project")
            .0
            .id
    }

    async fn seed_queue(db: &DatabaseConnection, priority: i16) -> Uuid {
        let id = Uuid::new_v4();
        queues::Entity::insert(queues::ActiveModel {
            id: Set(id),
            name: Set(format!("q{priority}")),
            priority: Set(priority),
            created_at: Set(Utc::now().into()),
        })
        .exec(db)
        .await
        .expect("seed queue");
        id
    }

    /// Directly stamp the queue link + in-queue priority on a task (bypassing
    /// `update_task`, which would trigger admission) so the picker can be tested
    /// in isolation.
    async fn enqueue(
        db: &DatabaseConnection,
        task_id: Uuid,
        queue_id: Option<Uuid>,
        priority: i16,
    ) {
        let mut active: tasks::ActiveModel = tasks::Entity::find_by_id(task_id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .into();
        active.queue_id = Set(queue_id);
        active.priority = Set(priority);
        active.update(db).await.unwrap();
    }

    fn mk(iid: u64) -> TriggerReason {
        TriggerReason::Issue {
            iid,
            title: format!("t{iid}"),
            description: String::new(),
            url: format!("http://x/{iid}"),
        }
    }

    /// pick_next_queued orders by queue priority, then in-queue priority, then
    /// age — and excludes unqueued, non-pending, and finished tasks.
    #[tokio::test]
    async fn pick_orders_and_excludes() {
        let Some((db, name, admin)) = fresh_db().await else {
            eprintln!("DATABASE_URL not set; skipping queue test");
            return;
        };
        let store = store_with(&db, LiveSessions::new(db.clone()), 3);
        let project_id = seed_project(&db).await;
        let q_hi = seed_queue(&db, 10).await;
        let q_lo = seed_queue(&db, 0).await;

        // Highest-priority queue wins regardless of in-queue priority.
        let in_lo_queue = store.create_task(mk(1), project_id).await.unwrap();
        enqueue(&db, in_lo_queue, Some(q_lo), 100).await;
        let in_hi_queue = store.create_task(mk(2), project_id).await.unwrap();
        enqueue(&db, in_hi_queue, Some(q_hi), 0).await;

        // An unqueued task is never picked.
        let unqueued = store.create_task(mk(3), project_id).await.unwrap();
        enqueue(&db, unqueued, None, 99).await;

        assert_eq!(store.pick_next_queued().await.unwrap(), Some(in_hi_queue));

        // Drop the high-priority queue's task to completed → excluded; the low
        // queue's task is next.
        store
            .set_states(in_hi_queue, "cold", TASK_COMPLETED)
            .await
            .unwrap();
        assert_eq!(store.pick_next_queued().await.unwrap(), Some(in_lo_queue));

        // Two tasks in the same queue + priority → older one first.
        let older = store.create_task(mk(4), project_id).await.unwrap();
        enqueue(&db, older, Some(q_hi), 5).await;
        let newer = store.create_task(mk(5), project_id).await.unwrap();
        enqueue(&db, newer, Some(q_hi), 5).await;
        assert_eq!(store.pick_next_queued().await.unwrap(), Some(older));

        // Finishing a queued task excludes it (finished_at set).
        store
            .finish_task(older, "cold", TASK_COMPLETED, None)
            .await
            .unwrap();
        assert_eq!(store.pick_next_queued().await.unwrap(), Some(newer));

        // Re-stamp the low task back to pending to prove the predicate, then
        // empty the queue and confirm None.
        for id in [in_lo_queue, newer] {
            store.set_states(id, "cold", TASK_COMPLETED).await.unwrap();
        }
        // in_lo_queue is `completed` (not pending) so excluded; newer too.
        // Only a pending+cold+unfinished+queued task qualifies.
        let only = store.create_task(mk(6), project_id).await.unwrap();
        enqueue(&db, only, Some(q_lo), 0).await;
        assert_eq!(store.pick_next_queued().await.unwrap(), Some(only));
        assert_eq!(
            tasks::Entity::find_by_id(only)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .task_state,
            TASK_PENDING
        );

        drop(store);
        drop(db);
        drop_db(&admin, &name).await;
    }

    /// try_admit_next is a no-op when every slot is already occupied: the queued
    /// task stays pending and uncfirmed.
    #[tokio::test]
    async fn admit_is_noop_when_full() {
        let Some((db, name, admin)) = fresh_db().await else {
            return;
        };
        let store = store_with(&db, LiveSessions::new(db.clone()), 2);
        let project_id = seed_project(&db).await;
        let q = seed_queue(&db, 0).await;

        // Fill every admission slot with dummy live registrations.
        for _ in 0..2 {
            let handle = tokio::spawn(async { std::future::pending::<()>().await }).abort_handle();
            store.running().register(Uuid::new_v4(), handle).await;
        }
        assert_eq!(store.running().len().await, 2);

        let queued = store.create_task(mk(1), project_id).await.unwrap();
        enqueue(&db, queued, Some(q), 0).await;

        store.try_admit_next().await;

        // Still pending+cold: no slot was free, so nothing was admitted.
        let t = tasks::Entity::find_by_id(queued)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t.task_state, TASK_PENDING);
        assert_eq!(t.agent_state, "cold");

        drop(store);
        drop(db);
        drop_db(&admin, &name).await;
    }
}
