//! DB-backed integration tests for the model-catalog uniqueness rule (issue
//! #46): the `alias` is globally unique while `model_id` may repeat across rows.
//! Exercises `ModelStore` against the real `idx_models_alias` constraint.
//!
//! Each test spins up a throwaway Postgres database off `DATABASE_URL`, runs the
//! migrations, and skips cleanly when no `DATABASE_URL` is configured.

use agent::models::{ModelStore, NewModel, ProviderStore};
use migration::MigratorTrait;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Statement};
use uuid::Uuid;

async fn fresh_db() -> Option<(DatabaseConnection, String, String)> {
    let base = std::env::var("DATABASE_URL").ok()?;
    let slash = base.rfind('/')?;
    let admin_url = format!("{}/postgres", &base[..slash]);
    let db_name = format!("agent_alias_{}", Uuid::new_v4().simple());

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

fn new_model(provider_id: Uuid, model_id: &str, alias: &str) -> NewModel {
    NewModel {
        provider_id,
        model_id: model_id.to_string(),
        alias: alias.to_string(),
        input_price: 0.0,
        output_price: 0.0,
        cache_write_price: 0.0,
        cache_read_price: 0.0,
        thinking: None,
        effort: None,
        is_default: false,
        unbound: false,
    }
}

#[tokio::test]
async fn same_model_id_distinct_alias_allowed_duplicate_alias_rejected() {
    let Some((db, name, admin)) = fresh_db().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };

    // The migration seeds exactly one provider (claude_code).
    let provider_id = ProviderStore::new(db.clone()).list().await.unwrap()[0].id;
    let models = ModelStore::new(db.clone());

    // Same model_id + same provider, different alias → both insert.
    models
        .create(new_model(provider_id, "claude-opus-4-8", "Opus (thinking)"))
        .await
        .expect("first model inserts");
    models
        .create(new_model(provider_id, "claude-opus-4-8", "Opus (unbound)"))
        .await
        .expect("same model_id with a different alias inserts");

    // A duplicate alias is rejected with the friendly message.
    let err = models
        .create(new_model(
            provider_id,
            "claude-haiku-4-8",
            "Opus (thinking)",
        ))
        .await
        .expect_err("duplicate alias must be rejected");
    assert_eq!(
        err.to_string(),
        "a model with alias \"Opus (thinking)\" already exists"
    );

    drop(models);
    drop(db);
    drop_db(&admin, &name).await;
}

#[tokio::test]
async fn update_to_taken_alias_rejected() {
    let Some((db, name, admin)) = fresh_db().await else {
        return;
    };

    let provider_id = ProviderStore::new(db.clone()).list().await.unwrap()[0].id;
    let models = ModelStore::new(db.clone());

    models
        .create(new_model(provider_id, "m1", "Alpha"))
        .await
        .expect("first model");
    let second = models
        .create(new_model(provider_id, "m2", "Beta"))
        .await
        .expect("second model");

    let err = models
        .update(
            second.id,
            agent::models::UpdateModel {
                alias: Some("Alpha".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect_err("renaming onto a taken alias must be rejected");
    assert_eq!(
        err.to_string(),
        "a model with alias \"Alpha\" already exists"
    );

    drop(models);
    drop(db);
    drop_db(&admin, &name).await;
}
