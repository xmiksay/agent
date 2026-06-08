use sea_orm_migration::prelude::*;

/// Per-service **trigger mode**: lets an issue trigger the agent by `label`, by
/// `assignee`, or `both` — not just assignee. `trigger_mode` defaults every
/// existing row to today's `assignee`; `trigger_label` names the label to watch
/// when the mode includes labels (empty until set). Label mode is how a GitHub
/// App — which can't be an issue assignee — gets triggered.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(GitServices::Table)
                    .add_column(
                        ColumnDef::new(GitServices::TriggerMode)
                            .string_len(16)
                            .not_null()
                            .default("assignee"),
                    )
                    .add_column(
                        ColumnDef::new(GitServices::TriggerLabel)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(GitServices::Table)
                    .drop_column(GitServices::TriggerMode)
                    .drop_column(GitServices::TriggerLabel)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
pub enum GitServices {
    Table,
    TriggerMode,
    TriggerLabel,
}
