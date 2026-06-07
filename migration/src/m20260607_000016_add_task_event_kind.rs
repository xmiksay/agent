use sea_orm_migration::prelude::*;

use crate::m20260605_000013_create_task_events::TaskEvents;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Existing rows predate the multi-kind hub and are all agent events.
        manager
            .alter_table(
                Table::alter()
                    .table(TaskEvents::Table)
                    .add_column(
                        ColumnDef::new(TaskEventsExt::Kind)
                            .text()
                            .not_null()
                            .default("event"),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(TaskEvents::Table)
                    .drop_column(TaskEventsExt::Kind)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum TaskEventsExt {
    Kind,
}
