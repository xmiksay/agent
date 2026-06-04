use sea_orm_migration::prelude::*;

use crate::m20260601_000003_create_projects::Projects;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(GitServices::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(GitServices::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(GitServices::Kind).string_len(16).not_null())
                    .col(
                        ColumnDef::new(GitServices::Slug)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(GitServices::DisplayName)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(ColumnDef::new(GitServices::BaseUrl).text().not_null())
                    .col(ColumnDef::new(GitServices::Token).text().not_null())
                    .col(
                        ColumnDef::new(GitServices::WebhookSecret)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(GitServices::BotUsername)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(GitServices::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(GitServices::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_git_services_slug")
                    .table(GitServices::Table)
                    .col(GitServices::Slug)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // At most one github service.
        let stmt = sea_orm_migration::sea_orm::Statement::from_string(
            sea_orm_migration::sea_orm::DatabaseBackend::Postgres,
            "CREATE UNIQUE INDEX idx_git_services_one_github \
             ON git_services (kind) WHERE kind = 'github'"
                .to_string(),
        );
        manager.get_connection().execute(stmt).await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Projects::Table)
                    .add_column(ColumnDef::new(ProjectsExt::GitServiceId).uuid().null())
                    .add_foreign_key(
                        TableForeignKey::new()
                            .name("fk_projects_git_service_id")
                            .from_tbl(Projects::Table)
                            .from_col(ProjectsExt::GitServiceId)
                            .to_tbl(GitServices::Table)
                            .to_col(GitServices::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_projects_git_service_id")
                    .table(Projects::Table)
                    .col(ProjectsExt::GitServiceId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_projects_git_service_id")
                    .table(Projects::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Projects::Table)
                    .drop_column(ProjectsExt::GitServiceId)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(GitServices::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum GitServices {
    Table,
    Id,
    Kind,
    Slug,
    DisplayName,
    BaseUrl,
    Token,
    WebhookSecret,
    BotUsername,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ProjectsExt {
    GitServiceId,
}
