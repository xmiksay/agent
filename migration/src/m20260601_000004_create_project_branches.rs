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
                    .table(ProjectBranches::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ProjectBranches::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ProjectBranches::ProjectId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ProjectBranches::BranchName)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ProjectBranches::BranchSlug)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(ColumnDef::new(ProjectBranches::IssueIid).big_integer())
                    .col(ColumnDef::new(ProjectBranches::PrIid).big_integer())
                    .col(
                        ColumnDef::new(ProjectBranches::Status)
                            .string_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ProjectBranches::CheckedOutAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ProjectBranches::LastUsedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ProjectBranches::Table, ProjectBranches::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_project_branches_project_slug")
                    .table(ProjectBranches::Table)
                    .col(ProjectBranches::ProjectId)
                    .col(ProjectBranches::BranchSlug)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_project_branches_issue")
                    .table(ProjectBranches::Table)
                    .col(ProjectBranches::ProjectId)
                    .col(ProjectBranches::IssueIid)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ProjectBranches::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum ProjectBranches {
    Table,
    Id,
    ProjectId,
    BranchName,
    BranchSlug,
    IssueIid,
    PrIid,
    Status,
    CheckedOutAt,
    LastUsedAt,
}
