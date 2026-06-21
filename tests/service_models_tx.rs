//! DB-backed integration test for the atomic trigger-mapping swap (issue #65):
//! `set_trigger_models` deletes the existing rows then re-inserts. If an insert
//! fails mid-swap, the whole operation must roll back so the service keeps its
//! prior mapping instead of being left with a wiped one.
//!
//! Each test spins up a throwaway Postgres database off `DATABASE_URL`, runs the
//! migrations, and skips cleanly when no `DATABASE_URL` is configured.

use std::collections::BTreeMap;

use agent::models::{ModelStore, NewModel, ProviderStore};
use agent::project::ProviderKind;
use agent::service::{NewService, ServiceStore};
use migration::MigratorTrait;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Statement};
use uuid::Uuid;

async fn fresh_db() -> Option<(DatabaseConnection, String, String)> {
    let base = std::env::var("DATABASE_URL").ok()?;
    let slash = base.rfind('/')?;
    let admin_url = format!("{}/postgres", &base[..slash]);
    let db_name = format!("agent_tx_{}", Uuid::new_v4().simple());

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

fn new_service() -> NewService {
    NewService {
        kind: ProviderKind::Github,
        slug: "acme".to_string(),
        display_name: "Acme".to_string(),
        base_url: "https://github.com".to_string(),
        token: "tok".to_string(),
        webhook_secret: "whsec".to_string(),
        bot_username: "bot".to_string(),
        autofire: false,
        auth_kind: Default::default(),
        app_credentials: None,
        trigger_mode: Default::default(),
        trigger_label: String::new(),
        models: None,
        triggers: None,
    }
}

#[tokio::test]
async fn failed_swap_rolls_back_to_prior_mapping() {
    let Some((db, name, admin)) = fresh_db().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };

    let provider_id = ProviderStore::new(db.clone()).list().await.unwrap()[0].id;
    let models = ModelStore::new(db.clone());
    let real_model = models
        .create(NewModel {
            provider_id,
            model_id: "claude-opus-4-8".to_string(),
            alias: "Opus".to_string(),
            input_price: 0.0,
            output_price: 0.0,
            cache_write_price: 0.0,
            cache_read_price: 0.0,
            thinking: None,
            effort: None,
            is_default: false,
            unbound: false,
        })
        .await
        .expect("seed model");

    let services = ServiceStore::new(db.clone());
    let service = services
        .create(new_service())
        .await
        .expect("create service");

    // Establish a known-good mapping.
    let mut good = BTreeMap::new();
    good.insert("issue".to_string(), real_model.id);
    services
        .set_trigger_models(service.id, &good)
        .await
        .expect("initial mapping");

    // Attempt a swap whose insert violates the model_id foreign key: the delete
    // runs first, then the insert fails. The transaction must roll back.
    let mut bad = BTreeMap::new();
    bad.insert("issue".to_string(), Uuid::new_v4()); // no such model
    services
        .set_trigger_models(service.id, &bad)
        .await
        .expect_err("swap with a dangling model_id must fail");

    // The prior mapping must survive intact — not be wiped by the failed delete.
    let after = services
        .trigger_models(service.id)
        .await
        .expect("reload mapping");
    assert_eq!(after, good, "failed swap should have rolled back");

    drop(services);
    drop(models);
    drop(db);
    drop_db(&admin, &name).await;
}

#[tokio::test]
async fn successful_swap_replaces_mapping() {
    let Some((db, name, admin)) = fresh_db().await else {
        return;
    };

    let provider_id = ProviderStore::new(db.clone()).list().await.unwrap()[0].id;
    let models = ModelStore::new(db.clone());
    let m1 = models
        .create(NewModel {
            provider_id,
            model_id: "claude-opus-4-8".to_string(),
            alias: "Opus".to_string(),
            input_price: 0.0,
            output_price: 0.0,
            cache_write_price: 0.0,
            cache_read_price: 0.0,
            thinking: None,
            effort: None,
            is_default: false,
            unbound: false,
        })
        .await
        .expect("model 1");
    let m2 = models
        .create(NewModel {
            provider_id,
            model_id: "claude-haiku-4-5".to_string(),
            alias: "Haiku".to_string(),
            input_price: 0.0,
            output_price: 0.0,
            cache_write_price: 0.0,
            cache_read_price: 0.0,
            thinking: None,
            effort: None,
            is_default: false,
            unbound: false,
        })
        .await
        .expect("model 2");

    let services = ServiceStore::new(db.clone());
    let service = services
        .create(new_service())
        .await
        .expect("create service");

    let mut first = BTreeMap::new();
    first.insert("issue".to_string(), m1.id);
    services
        .set_trigger_models(service.id, &first)
        .await
        .expect("first mapping");

    let mut second = BTreeMap::new();
    second.insert("review_mr".to_string(), m2.id);
    services
        .set_trigger_models(service.id, &second)
        .await
        .expect("second mapping");

    let after = services
        .trigger_models(service.id)
        .await
        .expect("reload mapping");
    assert_eq!(after, second, "swap should replace the whole mapping");

    drop(services);
    drop(models);
    drop(db);
    drop_db(&admin, &name).await;
}
