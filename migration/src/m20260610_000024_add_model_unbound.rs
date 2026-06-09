use sea_orm_migration::prelude::*;

/// `models.unbound` — a **dangerous** per-model flag. When set, a task running
/// this model is given `--dangerously-skip-permissions`: every tool call,
/// including arbitrary Bash, runs with no allowlist check and no operator
/// approval. Defaults `false` (the gated path). Surfaced prominently in the UI.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Models::Table)
                    .add_column(
                        ColumnDef::new(Models::Unbound)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Models::Table)
                    .drop_column(Models::Unbound)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Models {
    Table,
    Unbound,
}
