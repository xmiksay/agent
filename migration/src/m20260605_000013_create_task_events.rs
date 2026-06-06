use sea_orm_migration::prelude::*;

use crate::m20260415_000001_create_tasks::Tasks;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Append-only agent event stream. One row per event, keyed by
        // `(task_id, seq)`; the live hub batch-inserts ~100 at a time. The
        // composite PK doubles as the ordered read index. Cascades on task delete.
        manager
            .create_table(
                Table::create()
                    .table(TaskEvents::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(TaskEvents::TaskId).uuid().not_null())
                    .col(ColumnDef::new(TaskEvents::Seq).big_integer().not_null())
                    .col(ColumnDef::new(TaskEvents::Payload).json_binary().not_null())
                    .primary_key(
                        Index::create()
                            .col(TaskEvents::TaskId)
                            .col(TaskEvents::Seq),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(TaskEvents::Table, TaskEvents::TaskId)
                            .to(Tasks::Table, Tasks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(TaskEvents::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum TaskEvents {
    Table,
    TaskId,
    Seq,
    Payload,
}
