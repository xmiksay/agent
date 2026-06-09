use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{DatabaseBackend, Statement};

/// Rename the `git_services` table to `service` (singular) and the
/// `git_service_id` FK column to `service_id` on both `projects` and `tasks`,
/// keeping their indexes and FK constraints named consistently. Pure rename — no
/// data is lost. (`tasks.service_id` is dropped later by the run-table
/// normalization; here it's only renamed so the codebase can use one name.)
#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &[&str] = &[
    "ALTER TABLE git_services RENAME TO service",
    "ALTER TABLE projects RENAME COLUMN git_service_id TO service_id",
    "ALTER TABLE tasks RENAME COLUMN git_service_id TO service_id",
    "ALTER INDEX IF EXISTS idx_projects_git_service_id RENAME TO idx_projects_service_id",
    "ALTER INDEX IF EXISTS idx_tasks_git_service_id RENAME TO idx_tasks_service_id",
    "ALTER TABLE projects RENAME CONSTRAINT fk_projects_git_service_id TO fk_projects_service_id",
    "ALTER TABLE tasks RENAME CONSTRAINT fk_tasks_git_service_id TO fk_tasks_service_id",
];

const DOWN: &[&str] = &[
    "ALTER TABLE tasks RENAME CONSTRAINT fk_tasks_service_id TO fk_tasks_git_service_id",
    "ALTER TABLE projects RENAME CONSTRAINT fk_projects_service_id TO fk_projects_git_service_id",
    "ALTER INDEX IF EXISTS idx_tasks_service_id RENAME TO idx_tasks_git_service_id",
    "ALTER INDEX IF EXISTS idx_projects_service_id RENAME TO idx_projects_git_service_id",
    "ALTER TABLE tasks RENAME COLUMN service_id TO git_service_id",
    "ALTER TABLE projects RENAME COLUMN service_id TO git_service_id",
    "ALTER TABLE service RENAME TO git_services",
];

async fn run(manager: &SchemaManager<'_>, stmts: &[&str]) -> Result<(), DbErr> {
    let db = manager.get_connection();
    for sql in stmts {
        db.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            String::from(*sql),
        ))
        .await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        run(manager, UP).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        run(manager, DOWN).await
    }
}
