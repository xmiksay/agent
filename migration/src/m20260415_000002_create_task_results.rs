use sea_orm_migration::prelude::*;

use crate::m20260415_000001_create_tasks::Tasks;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(TaskResults::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TaskResults::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TaskResults::TaskId)
                            .uuid()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(TaskResults::CostUsd).double().not_null())
                    .col(
                        ColumnDef::new(TaskResults::InputTokens)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TaskResults::OutputTokens)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(TaskResults::NumTurns).integer().not_null())
                    .col(ColumnDef::new(TaskResults::IsError).boolean().not_null())
                    .col(ColumnDef::new(TaskResults::ResultText).text().not_null())
                    .col(ColumnDef::new(TaskResults::SessionId).string().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(TaskResults::Table, TaskResults::TaskId)
                            .to(Tasks::Table, Tasks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(TaskResults::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum TaskResults {
    Table,
    Id,
    TaskId,
    CostUsd,
    InputTokens,
    OutputTokens,
    NumTurns,
    IsError,
    ResultText,
    SessionId,
}
