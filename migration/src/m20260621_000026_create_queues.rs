use sea_orm_migration::prelude::*;

/// The **task queue**. A `queues` row is an operator-orderable backlog bucket
/// (its own `priority` orders queue-vs-queue); each task carries an optional
/// `queue_id` FK plus an in-queue `priority`. The scheduler pulls the next
/// `pending` task — ordered by `queues.priority`, then `tasks.priority`, then
/// age — whenever a slot frees, so confirmed work no longer has to spawn eagerly.
///
/// Modelled as a FK + scalar rather than a `tasks uuid[]` array on the queue:
/// the link a FK already expresses isn't duplicated, "filter out completed" is a
/// plain `task_state` predicate, and mutations use the standard SeaORM
/// `ActiveModel + Set(...)` pattern (no racy read-modify-write of an array).
///
/// The FK degrades safely: deleting a queue nulls its tasks' `queue_id`
/// (un-queues them), mirroring the `tasks.model_id` precedent.
#[derive(DeriveMigrationName)]
pub struct Migration;

/// Stable id for the seeded default queue, so fresh installs have one bucket to
/// enqueue into without an operator setup step (mirrors the seeded provider).
const DEFAULT_QUEUE_ID: &str = "00000000-0000-0000-0000-00000000c0e0";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Queues::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Queues::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Queues::Name).text().not_null())
                    .col(
                        ColumnDef::new(Queues::Priority)
                            .small_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Queues::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Seed one default queue so a fresh install can enqueue with no setup.
        let seed = sea_orm_migration::sea_orm::Statement::from_string(
            sea_orm_migration::sea_orm::DatabaseBackend::Postgres,
            format!(
                "INSERT INTO queues (id, name, priority, created_at) \
                 VALUES ('{DEFAULT_QUEUE_ID}', 'Default', 0, now())"
            ),
        );
        manager.get_connection().execute(seed).await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .add_column(ColumnDef::new(Tasks::QueueId).uuid().null())
                    .add_column(
                        ColumnDef::new(Tasks::Priority)
                            .small_integer()
                            .not_null()
                            .default(0),
                    )
                    .add_foreign_key(
                        TableForeignKey::new()
                            .name("fk_tasks_queue_id")
                            .from_tbl(Tasks::Table)
                            .from_col(Tasks::QueueId)
                            .to_tbl(Queues::Table)
                            .to_col(Queues::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        // The scheduler orders by (queue, in-queue priority); index the pair.
        manager
            .create_index(
                Index::create()
                    .name("idx_tasks_queue_priority")
                    .table(Tasks::Table)
                    .col(Tasks::QueueId)
                    .col(Tasks::Priority)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_tasks_queue_priority")
                    .table(Tasks::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .drop_column(Tasks::QueueId)
                    .drop_column(Tasks::Priority)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Queues::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Queues {
    Table,
    Id,
    Name,
    Priority,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Tasks {
    Table,
    QueueId,
    Priority,
}
