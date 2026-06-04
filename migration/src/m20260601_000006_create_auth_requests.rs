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
                    .table(AuthRequests::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AuthRequests::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AuthRequests::TaskId).uuid().not_null())
                    .col(ColumnDef::new(AuthRequests::RequestedOp).text().not_null())
                    .col(
                        ColumnDef::new(AuthRequests::PromptToOperator)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AuthRequests::Status)
                            .string_len(16)
                            .not_null(),
                    )
                    .col(ColumnDef::new(AuthRequests::OperatorReply).text())
                    .col(
                        ColumnDef::new(AuthRequests::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AuthRequests::ResolvedAt).timestamp_with_time_zone())
                    .foreign_key(
                        ForeignKey::create()
                            .from(AuthRequests::Table, AuthRequests::TaskId)
                            .to(Tasks::Table, Tasks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_auth_requests_status")
                    .table(AuthRequests::Table)
                    .col(AuthRequests::Status)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AuthRequests::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum AuthRequests {
    Table,
    Id,
    TaskId,
    RequestedOp,
    PromptToOperator,
    Status,
    OperatorReply,
    CreatedAt,
    ResolvedAt,
}
