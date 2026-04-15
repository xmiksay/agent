use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Tasks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Tasks::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Tasks::Status).string_len(32).not_null())
                    .col(ColumnDef::new(Tasks::TriggerType).string_len(64).not_null())
                    .col(ColumnDef::new(Tasks::TriggerData).json_binary().not_null())
                    .col(ColumnDef::new(Tasks::ProjectPath).string().not_null())
                    .col(ColumnDef::new(Tasks::GitUrl).string().not_null())
                    .col(ColumnDef::new(Tasks::DefaultBranch).string().not_null())
                    .col(
                        ColumnDef::new(Tasks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Tasks::StartedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Tasks::FinishedAt).timestamp_with_time_zone())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Tasks::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum Tasks {
    Table,
    Id,
    Status,
    TriggerType,
    TriggerData,
    ProjectPath,
    GitUrl,
    DefaultBranch,
    CreatedAt,
    StartedAt,
    FinishedAt,
}
