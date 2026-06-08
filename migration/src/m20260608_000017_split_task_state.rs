use sea_orm_migration::prelude::*;

use crate::m20260415_000001_create_tasks::Tasks;

/// Split the overloaded `tasks.status` into two orthogonal axes:
///   * `task_state` — NEW persisted operator lifecycle (pending|working_on|
///     completed|failed), freely operator-overridable.
///   * `agent_state` — the old `status` column RENAMED, narrowed to the durable
///     subset (cold|pending|failed). The volatile warm/running distinction is
///     overlaid at read time from the live hub, not persisted.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. Add task_state (default pending so the NOT NULL holds for the
        //    backfill that immediately overwrites it).
        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .add_column(
                        ColumnDef::new(TasksExt::TaskState)
                            .string_len(32)
                            .not_null()
                            .default("pending"),
                    )
                    .to_owned(),
            )
            .await?;

        let db = manager.get_connection();

        // 2. Backfill task_state from the old `status` column (still named
        //    `status` here). running → working_on; killed → failed; the rest map
        //    by name.
        db.execute_unprepared(
            "UPDATE tasks SET task_state = CASE status \
                WHEN 'pending'   THEN 'pending' \
                WHEN 'running'   THEN 'working_on' \
                WHEN 'completed' THEN 'completed' \
                WHEN 'failed'    THEN 'failed' \
                WHEN 'killed'    THEN 'failed' \
                ELSE 'failed' END",
        )
        .await?;

        // 3. Rename status → agent_state.
        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .rename_column(Tasks::Status, TasksExt::AgentState)
                    .to_owned(),
            )
            .await?;

        // 4. Narrow agent_state to the durable set (cold|pending|failed).
        //    running/completed/killed collapse: pending stays pending, failed/
        //    killed → failed, everything else (running, completed) → cold.
        db.execute_unprepared(
            "UPDATE tasks SET agent_state = CASE agent_state \
                WHEN 'pending' THEN 'pending' \
                WHEN 'failed'  THEN 'failed' \
                WHEN 'killed'  THEN 'failed' \
                ELSE 'cold' END",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Best-effort reverse: rename agent_state back to status, drop task_state.
        // The original fine-grained status values are not recoverable.
        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .rename_column(TasksExt::AgentState, Tasks::Status)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .drop_column(TasksExt::TaskState)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum TasksExt {
    TaskState,
    AgentState,
}
