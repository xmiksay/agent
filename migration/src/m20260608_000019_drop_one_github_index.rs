use sea_orm_migration::prelude::*;

/// Drop the partial unique index that limited the table to a single `github`
/// service. Multiple GitHub services are now allowed (e.g. a PAT service and a
/// GitHub App service, or github.com + GHES, or several orgs) — they're already
/// distinguished by their unique `slug` and per-service webhook route.
#[derive(DeriveMigrationName)]
pub struct Migration;

const INDEX_NAME: &str = "idx_git_services_one_github";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let stmt = sea_orm_migration::sea_orm::Statement::from_string(
            sea_orm_migration::sea_orm::DatabaseBackend::Postgres,
            format!("DROP INDEX IF EXISTS {INDEX_NAME}"),
        );
        manager.get_connection().execute(stmt).await.map(|_| ())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let stmt = sea_orm_migration::sea_orm::Statement::from_string(
            sea_orm_migration::sea_orm::DatabaseBackend::Postgres,
            format!(
                "CREATE UNIQUE INDEX {INDEX_NAME} ON git_services (kind) WHERE kind = 'github'"
            ),
        );
        manager.get_connection().execute(stmt).await.map(|_| ())
    }
}
