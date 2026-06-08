use sea_orm_migration::prelude::*;

use crate::m20260601_000006_create_auth_requests::AuthRequests;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AuthRequests::Table)
                    .add_column(
                        ColumnDef::new(AuthRequestsExt::Metadata)
                            .json_binary()
                            .null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AuthRequests::Table)
                    .drop_column(AuthRequestsExt::Metadata)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum AuthRequestsExt {
    Metadata,
}
