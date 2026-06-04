use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Projects::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Projects::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Projects::Provider).string_len(16).not_null())
                    .col(
                        ColumnDef::new(Projects::ProjectSlug)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Projects::FullName)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Projects::RemoteUrl).text().not_null())
                    .col(
                        ColumnDef::new(Projects::DefaultBranch)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Projects::MyUsername)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Projects::AllowedOperations)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Projects::Notes)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(Projects::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Projects::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_projects_provider_slug")
                    .table(Projects::Table)
                    .col(Projects::Provider)
                    .col(Projects::ProjectSlug)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Projects::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum Projects {
    Table,
    Id,
    Provider,
    ProjectSlug,
    FullName,
    RemoteUrl,
    DefaultBranch,
    MyUsername,
    AllowedOperations,
    Notes,
    CreatedAt,
    UpdatedAt,
}
