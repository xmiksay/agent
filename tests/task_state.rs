//! DB-backed integration tests for the public task-state surface: creation
//! seeds the two axes, the `task_state` PATCH contract (validation + allowed on
//! any state while branch edits stay pending-gated), and the `task_state` SQL
//! list filter. The full create→run→warm lifecycle (which drives `pub(crate)`
//! run-loop helpers and the live hub) is covered by the in-lib `#[cfg(test)]`
//! module in `src/jobs/lifecycle.rs`.
//!
//! Each test spins up its own throwaway Postgres database off `DATABASE_URL`,
//! runs the migrations, and exercises the store directly — no claude runner.

use std::sync::Arc;

use agent::auth::store::AuthStore;
use agent::auth::waiter::AuthWaiter;
use agent::config::Config;
use agent::service::{ServiceStore, NewService};
use agent::jobs::hub::LiveSessions;
use agent::jobs::lifecycle::derive_agent_state;
use agent::jobs::registry::RunningTasks;
use agent::jobs::store::{TaskEdits, TaskStore};
use agent::jobs::types::TriggerReason;
use agent::project::{ProjectStore, ProviderKind};
use agent::provider::ProviderRegistry;
use agent::workspace::Workspace;
use migration::MigratorTrait;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Statement};
use uuid::Uuid;

/// Connect to the admin DB, create a fresh per-test database, connect to it, and
/// run migrations. Returns the connection plus the names needed to drop it.
/// Returns None if no DATABASE_URL is configured (test then skips).
async fn fresh_db() -> Option<(DatabaseConnection, String, String)> {
    let base = std::env::var("DATABASE_URL").ok()?;
    let slash = base.rfind('/')?;
    let admin_url = format!("{}/postgres", &base[..slash]);
    let db_name = format!("agent_test_{}", Uuid::new_v4().simple());

    let admin = Database::connect(&admin_url).await.ok()?;
    admin
        .execute(Statement::from_string(
            admin.get_database_backend(),
            format!("CREATE DATABASE \"{db_name}\""),
        ))
        .await
        .expect("create test db");

    let test_url = format!("{}/{db_name}", &base[..slash]);
    let conn = Database::connect(&test_url).await.expect("connect test db");
    migration::Migrator::up(&conn, None)
        .await
        .expect("run migrations");
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

fn build_store(db: &DatabaseConnection) -> Arc<TaskStore> {
    let config = Config {
        api_bearer_token: None,
        database_url: String::new(),
        repo_base_path: "/tmp/agent-test".to_string(),
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
        LiveSessions::new(db.clone()),
        Arc::new(AuthStore::new(db.clone())),
        AuthWaiter::new(),
    ))
}

fn issue_trigger(iid: u64) -> TriggerReason {
    TriggerReason::Issue {
        iid,
        title: format!("t{iid}"),
        description: String::new(),
        url: format!("http://example.test/issues/{iid}"),
    }
}

/// Insert a service row so `tasks.service_id` satisfies its FK.
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

async fn new_task(store: &Arc<TaskStore>, service_id: Uuid, iid: u64) -> Uuid {
    store
        .create_task(
            issue_trigger(iid),
            service_id,
            ProviderKind::Github,
            None,
            "acme/widgets".to_string(),
            "git@example.test:acme/widgets.git".to_string(),
            "main".to_string(),
        )
        .await
        .expect("create task")
}

fn patch_task_state(ts: &str) -> TaskEdits {
    TaskEdits {
        branch: None,
        default_branch: None,
        task_state: Some(ts.to_string()),
    }
}

#[tokio::test]
async fn create_seeds_two_axes() {
    let Some((db, name, admin)) = fresh_db().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };
    let store = build_store(&db);
    let svc = seed_service(&db).await;

    let id = new_task(&store, svc, 1).await;
    let (t, _) = store.get_task(id).await.unwrap().unwrap();
    assert_eq!(t.agent_state, "cold", "durable agent_state after create");
    assert_eq!(t.task_state, "pending", "task_state after create");
    // No live channel → derives off the durable backing.
    assert_eq!(derive_agent_state(&t.agent_state, id, store.hub()), "cold");

    drop(store);
    drop(db);
    drop_db(&admin, &name).await;
}

#[tokio::test]
async fn patch_validation_and_branch_gate() {
    let Some((db, name, admin)) = fresh_db().await else {
        return;
    };
    let store = build_store(&db);
    let svc = seed_service(&db).await;
    let id = new_task(&store, svc, 1).await;

    // Invalid value rejected.
    let err = store
        .update_task(id, patch_task_state("nope"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("invalid task_state"), "{err}");

    // All four valid values accepted (task starts pending).
    for valid in ["pending", "working_on", "completed", "failed"] {
        store
            .update_task(id, patch_task_state(valid))
            .await
            .unwrap_or_else(|e| panic!("valid task_state {valid} rejected: {e}"));
        let (t, _) = store.get_task(id).await.unwrap().unwrap();
        assert_eq!(t.task_state, valid);
    }

    // task_state is now "failed" (not pending): a task_state PATCH is still
    // allowed on any state...
    store
        .update_task(id, patch_task_state("completed"))
        .await
        .expect("task_state PATCH allowed on non-pending task");

    // ...but a branch edit is rejected once the task is past pending.
    let err = store
        .update_task(
            id,
            TaskEdits {
                branch: Some("feature".to_string()),
                default_branch: None,
                task_state: None,
            },
        )
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("can only edit branch fields"),
        "branch edit on non-pending task must be rejected: {err}"
    );

    drop(store);
    drop(db);
    drop_db(&admin, &name).await;
}

#[tokio::test]
async fn list_filters_by_task_state() {
    let Some((db, name, admin)) = fresh_db().await else {
        return;
    };
    let store = build_store(&db);
    let svc = seed_service(&db).await;

    let pend = new_task(&store, svc, 1).await;
    let work = new_task(&store, svc, 2).await;
    let done = new_task(&store, svc, 3).await;
    store
        .update_task(work, patch_task_state("working_on"))
        .await
        .unwrap();
    store
        .update_task(done, patch_task_state("completed"))
        .await
        .unwrap();

    let working = store.list_tasks(Some("working_on")).await.unwrap();
    assert_eq!(working.len(), 1);
    assert_eq!(working[0].id, work);

    let pending = store.list_tasks(Some("pending")).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, pend);

    let all = store.list_tasks(None).await.unwrap();
    assert_eq!(all.len(), 3);
    // Unique ids and all three task_states represented.
    assert!(
        [pend, work, done]
            .iter()
            .all(|id| all.iter().any(|t| &t.id == id))
    );

    drop(store);
    drop(db);
    drop_db(&admin, &name).await;
}
