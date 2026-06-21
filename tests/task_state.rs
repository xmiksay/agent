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

use agent::auth::resolve::resolve_and_publish;
use agent::auth::store::{AuthStatus, AuthStore};
use agent::auth::waiter::AuthWaiter;
use agent::config::Config;
use agent::jobs::hub::LiveSessions;
use agent::jobs::lifecycle::derive_agent_state;
use agent::jobs::registry::RunningTasks;
use agent::jobs::store::{TaskEdits, TaskStore};
use agent::jobs::types::TriggerReason;
use agent::project::{NewProjectConfig, ProjectStore, ProviderKind};
use agent::provider::ProviderRegistry;
use agent::service::{NewService, ServiceStore};
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
            models: None,
        })
        .await
        .expect("seed service")
        .id
}

/// Seed a service + a project linked to it, returning the project id (the only
/// link a task needs now).
async fn seed_project(db: &DatabaseConnection) -> Uuid {
    let service_id = seed_service(db).await;
    ProjectStore::new(db.clone())
        .upsert_project(NewProjectConfig {
            provider: ProviderKind::Github,
            service_id,
            project_slug: format!("p-{}", Uuid::new_v4().simple()),
            full_name: "acme/widgets".to_string(),
            remote_url: "git@example.test:acme/widgets.git".to_string(),
            default_branch: "main".to_string(),
            my_username: "bot".to_string(),
        })
        .await
        .expect("seed project")
        .0
        .id
}

async fn new_task(store: &Arc<TaskStore>, project_id: Uuid, iid: u64) -> Uuid {
    store
        .create_task(issue_trigger(iid), project_id)
        .await
        .expect("create task")
}

fn patch_task_state(ts: &str) -> TaskEdits {
    TaskEdits {
        task_state: Some(ts.to_string()),
        ..Default::default()
    }
}

#[tokio::test]
async fn create_seeds_two_axes() {
    let Some((db, name, admin)) = fresh_db().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };
    let store = build_store(&db);
    let project_id = seed_project(&db).await;

    let id = new_task(&store, project_id, 1).await;
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
    let project_id = seed_project(&db).await;
    let id = new_task(&store, project_id, 1).await;

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
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("can only edit the branch"),
        "branch edit on non-pending task must be rejected: {err}"
    );

    // ...but the model override IS editable on a non-pending task — it's read
    // fresh at each spawn, so it applies on the next run/resume (#51).
    store
        .update_task(
            id,
            TaskEdits {
                model_id: Some(None),
                ..Default::default()
            },
        )
        .await
        .expect("model edit must be allowed on a non-pending task (#51)");

    drop(store);
    drop(db);
    drop_db(&admin, &name).await;
}

#[tokio::test]
async fn issue_task_dedup_and_description_update() {
    let Some((db, name, admin)) = fresh_db().await else {
        return;
    };
    let store = build_store(&db);
    let project_id = seed_project(&db).await;

    let id = new_task(&store, project_id, 7).await;

    // The issue's task is found by iid (the dedup anchor); an untracked issue
    // returns nothing, so dispatch falls through to creating a fresh task.
    let found = store.find_issue_task(project_id, 7).await.unwrap();
    assert_eq!(found.map(|t| t.id), Some(id));
    assert!(
        store
            .find_issue_task(project_id, 8)
            .await
            .unwrap()
            .is_none()
    );

    // An edited issue webhook rewrites the stored title + description in place
    // instead of creating a second task.
    store
        .update_issue_description(id, "new title", "new body")
        .await
        .expect("update issue description");

    let (t, _) = store.get_task(id).await.unwrap().unwrap();
    match serde_json::from_value::<TriggerReason>(t.trigger_data).unwrap() {
        TriggerReason::Issue {
            iid,
            title,
            description,
            ..
        } => {
            assert_eq!(iid, 7);
            assert_eq!(title, "new title");
            assert_eq!(description, "new body");
        }
        other => panic!("expected issue trigger, got {other:?}"),
    }

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
    let project_id = seed_project(&db).await;

    let pend = new_task(&store, project_id, 1).await;
    let work = new_task(&store, project_id, 2).await;
    let done = new_task(&store, project_id, 3).await;
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

/// The model-selector rework: a new task inherits the owning service's mapping
/// for its trigger type, and an unmapped trigger type falls back to the global
/// default model.
#[tokio::test]
async fn task_inherits_service_trigger_model_then_global_default() {
    use agent::models::{ModelStore, NewModel, ProviderStore};
    use std::collections::BTreeMap;

    let Some((db, name, admin)) = fresh_db().await else {
        return;
    };
    let store = build_store(&db);

    let service_id = seed_service(&db).await;
    let project_id = ProjectStore::new(db.clone())
        .upsert_project(NewProjectConfig {
            provider: ProviderKind::Github,
            service_id,
            project_slug: format!("p-{}", Uuid::new_v4().simple()),
            full_name: "acme/widgets".to_string(),
            remote_url: "git@example.test:acme/widgets.git".to_string(),
            default_branch: "main".to_string(),
            my_username: "bot".to_string(),
        })
        .await
        .expect("seed project")
        .0
        .id;

    // The migration seeds exactly one provider (claude_code).
    let provider_id = ProviderStore::new(db.clone()).list().await.unwrap()[0].id;
    let models = ModelStore::new(db.clone());
    let new_model = |model_id: &str, alias: &str, is_default: bool| NewModel {
        provider_id,
        model_id: model_id.to_string(),
        alias: alias.to_string(),
        input_price: 0.0,
        output_price: 0.0,
        cache_write_price: 0.0,
        cache_read_price: 0.0,
        thinking: None,
        effort: None,
        is_default,
        unbound: false,
    };
    let issue_model = models
        .create(new_model("opus", "Opus", false))
        .await
        .unwrap();
    let default_model = models
        .create(new_model("haiku", "Haiku", true))
        .await
        .unwrap();

    // Map the `issue` trigger type on the service to the issue model.
    let mut map = BTreeMap::new();
    map.insert("issue".to_string(), issue_model.id);
    ServiceStore::new(db.clone())
        .set_trigger_models(service_id, &map)
        .await
        .unwrap();

    // An issue task inherits the mapped model.
    let issue_id = new_task(&store, project_id, 1).await;
    let (t, _) = store.get_task(issue_id).await.unwrap().unwrap();
    assert_eq!(
        t.model_id,
        Some(issue_model.id),
        "issue task inherits the service's per-trigger-type model"
    );

    // An MR-comment task (unmapped trigger type) falls back to the global default.
    let mr_id = store
        .create_task(
            TriggerReason::MRComment {
                mr_iid: 7,
                comment: "hi".to_string(),
                source_branch: "feature".to_string(),
                url: "http://example.test/mr/7".to_string(),
            },
            project_id,
        )
        .await
        .unwrap();
    let (mt, _) = store.get_task(mr_id).await.unwrap().unwrap();
    assert_eq!(
        mt.model_id,
        Some(default_model.id),
        "unmapped trigger type inherits the global default model"
    );

    drop(store);
    drop(db);
    drop_db(&admin, &name).await;
}

/// Bulk-deny resolves every targeted pending row to `denied` and is idempotent:
/// once resolved, a second pass finds nothing pending. Mirrors the store work
/// the `/api/auth_requests/bulk_resolve` handler does (list-pending → resolve
/// loop via the shared `resolve_and_publish`).
#[tokio::test]
async fn bulk_resolve_denies_all_pending_then_is_idempotent() {
    let Some((db, name, admin)) = fresh_db().await else {
        return;
    };
    let store = build_store(&db);
    let project_id = seed_project(&db).await;
    let task_id = new_task(&store, project_id, 1).await;

    let auth_store = AuthStore::new(db.clone());
    let waiter = AuthWaiter::new();

    for i in 0..3 {
        auth_store
            .create_pending(task_id, format!("op-{i}"), "prompt".into(), None)
            .await
            .expect("create pending");
    }

    let targets: Vec<Uuid> = auth_store
        .list_filtered(Some(AuthStatus::Pending), None)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(targets.len(), 3);

    for id in &targets {
        resolve_and_publish(
            &auth_store,
            &waiter,
            store.hub(),
            *id,
            AuthStatus::Denied,
            Some("bulk".into()),
        )
        .await
        .expect("resolve");
    }

    for id in &targets {
        let r = auth_store.get(*id).await.unwrap().unwrap();
        assert_eq!(r.status, AuthStatus::Denied);
        assert!(r.resolved_at.is_some(), "resolved_at stamped");
    }
    assert!(
        auth_store
            .list_filtered(Some(AuthStatus::Pending), None)
            .await
            .unwrap()
            .is_empty(),
        "no pending rows remain after bulk deny (idempotent on re-run)"
    );

    drop(store);
    drop(db);
    drop_db(&admin, &name).await;
}

/// AskUserQuestion approval carrier (#48): the resolve route stringifies the
/// operator's structured `answers` into `operator_reply`, and the parked
/// question handler parses it straight back. This exercises that round-trip
/// through the store the same way `resolve_auth_request` does — answers in as a
/// JSON string, answers out unchanged.
#[tokio::test]
async fn question_answers_round_trip_through_operator_reply() {
    let Some((db, name, admin)) = fresh_db().await else {
        return;
    };
    let store = build_store(&db);
    let project_id = seed_project(&db).await;
    let task_id = new_task(&store, project_id, 1).await;

    let auth_store = AuthStore::new(db.clone());
    let waiter = AuthWaiter::new();
    let questions = serde_json::json!([
        { "question": "Which DB?", "options": [{ "label": "Postgres" }] },
        { "question": "Which caches?", "multiSelect": true, "options": [{ "label": "Redis" }] },
    ]);
    let auth = auth_store
        .create_pending(
            task_id,
            "AskUserQuestion".into(),
            "prompt".into(),
            Some(serde_json::json!({ "questions": questions })),
        )
        .await
        .expect("create pending");

    // The operator answers — a custom string for one question, a label list for
    // the other. The route persists this as a stringified JSON object.
    let answers = serde_json::json!({
        "Which DB?": "a bespoke answer",
        "Which caches?": ["Redis"],
    });
    let resolved = resolve_and_publish(
        &auth_store,
        &waiter,
        store.hub(),
        auth.id,
        AuthStatus::Approved,
        Some(answers.to_string()),
    )
    .await
    .expect("resolve question");

    assert_eq!(resolved.status, AuthStatus::Approved);
    let stored = resolved.operator_reply.expect("answers stored in reply");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stored).unwrap(),
        answers,
        "operator_reply round-trips the structured answers object"
    );

    drop(store);
    drop(db);
    drop_db(&admin, &name).await;
}

/// The finite-timeout auto-deny path: the same `resolve_and_publish` the runner
/// calls on timeout flips a pending row to denied with the timeout message, so
/// the row leaves `pending` and stops being re-surfaced (#45).
#[tokio::test]
async fn timeout_path_resolves_row_to_denied() {
    let Some((db, name, admin)) = fresh_db().await else {
        return;
    };
    let store = build_store(&db);
    let project_id = seed_project(&db).await;
    let task_id = new_task(&store, project_id, 1).await;

    let auth_store = AuthStore::new(db.clone());
    let waiter = AuthWaiter::new();
    let auth = auth_store
        .create_pending(task_id, "rm -rf".into(), "prompt".into(), None)
        .await
        .expect("create pending");

    let resolved = resolve_and_publish(
        &auth_store,
        &waiter,
        store.hub(),
        auth.id,
        AuthStatus::Denied,
        Some("Operator approval timed out".into()),
    )
    .await
    .expect("timeout resolve");

    assert_eq!(resolved.status, AuthStatus::Denied);
    assert_eq!(
        resolved.operator_reply.as_deref(),
        Some("Operator approval timed out")
    );
    assert!(resolved.resolved_at.is_some());

    drop(store);
    drop(db);
    drop_db(&admin, &name).await;
}
