use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{DatabaseBackend, Statement};

/// Normalize the run tables:
/// 1. Drop the columns on `tasks` that are functionally dependent on
///    `project_id` (`git_url`, `default_branch`, `provider`, `project_path`,
///    `service_id`) — the runner resolves them via `project_id → projects →
///    service`. `project_id` becomes mandatory.
/// 2. `task_results` → `task_sessions`, turned 1:N (drop the `unique(task_id)`,
///    add `created_at` for ordering): a task accumulates one session row per
///    agent run, not a single overwritten result.
/// 3. `task_events` → `events`.
///
/// Run data is ephemeral — no backfill; rows that can't satisfy the new shape
/// (a task with no project) are dropped.
#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &[&str] = &[
    "ALTER TABLE tasks DROP COLUMN IF EXISTS git_url",
    "ALTER TABLE tasks DROP COLUMN IF EXISTS default_branch",
    "ALTER TABLE tasks DROP COLUMN IF EXISTS provider",
    "ALTER TABLE tasks DROP COLUMN IF EXISTS project_path",
    "ALTER TABLE tasks DROP COLUMN IF EXISTS service_id",
    "DELETE FROM tasks WHERE project_id IS NULL",
    "ALTER TABLE tasks DROP CONSTRAINT IF EXISTS fk_tasks_project_id",
    "ALTER TABLE tasks ALTER COLUMN project_id SET NOT NULL",
    "ALTER TABLE tasks ADD CONSTRAINT fk_tasks_project_id FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE",
    "ALTER TABLE task_results RENAME TO task_sessions",
    // Drop whatever unique constraint sits on task_sessions (was unique(task_id)).
    "DO $$ DECLARE c text; BEGIN SELECT conname INTO c FROM pg_constraint WHERE conrelid = 'task_sessions'::regclass AND contype = 'u'; IF c IS NOT NULL THEN EXECUTE 'ALTER TABLE task_sessions DROP CONSTRAINT ' || quote_ident(c); END IF; END $$",
    "ALTER TABLE task_sessions ADD COLUMN IF NOT EXISTS created_at timestamptz NOT NULL DEFAULT now()",
    "CREATE INDEX IF NOT EXISTS idx_task_sessions_task_id ON task_sessions (task_id)",
    "ALTER TABLE task_events RENAME TO events",
];

const DOWN: &[&str] = &[
    "ALTER TABLE events RENAME TO task_events",
    "DROP INDEX IF EXISTS idx_task_sessions_task_id",
    "ALTER TABLE task_sessions DROP COLUMN IF EXISTS created_at",
    "ALTER TABLE task_sessions ADD CONSTRAINT task_results_task_id_key UNIQUE (task_id)",
    "ALTER TABLE task_sessions RENAME TO task_results",
    "ALTER TABLE tasks DROP CONSTRAINT IF EXISTS fk_tasks_project_id",
    "ALTER TABLE tasks ALTER COLUMN project_id DROP NOT NULL",
    "ALTER TABLE tasks ADD CONSTRAINT fk_tasks_project_id FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL",
    "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS service_id uuid",
    "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS project_path varchar NOT NULL DEFAULT ''",
    "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS provider varchar NOT NULL DEFAULT ''",
    "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS default_branch varchar NOT NULL DEFAULT ''",
    "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS git_url varchar NOT NULL DEFAULT ''",
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
