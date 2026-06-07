use sea_orm_migration::prelude::*;

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
                        ColumnDef::new(GitServices::Autofire)
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
                    .table(GitServices::Table)
                    .drop_column(GitServices::Autofire)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
pub enum GitServices {
    Table,
    Autofire,
}
