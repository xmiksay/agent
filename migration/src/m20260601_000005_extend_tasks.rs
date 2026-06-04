use sea_orm_migration::prelude::*;

use crate::m20260415_000001_create_tasks::Tasks;
use crate::m20260601_000003_create_projects::Projects;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .add_column(
                        ColumnDef::new(TasksExt::Provider)
                            .string_len(16)
                            .not_null()
                            .default("gitlab"),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .add_column(ColumnDef::new(TasksExt::Branch).string_len(255).null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .add_column(ColumnDef::new(TasksExt::ProjectId).uuid().null())
                    .add_foreign_key(
                        TableForeignKey::new()
                            .name("fk_tasks_project_id")
                            .from_tbl(Tasks::Table)
                            .from_col(TasksExt::ProjectId)
                            .to_tbl(Projects::Table)
                            .to_col(Projects::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tasks_provider_path_branch")
                    .table(Tasks::Table)
                    .col(TasksExt::Provider)
                    .col(Tasks::ProjectPath)
                    .col(TasksExt::Branch)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_tasks_provider_path_branch")
                    .table(Tasks::Table)
                    .to_owned(),
            )
            .await?;
        for col in [TasksExt::ProjectId, TasksExt::Branch, TasksExt::Provider] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Tasks::Table)
                        .drop_column(col)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum TasksExt {
    Provider,
    Branch,
    ProjectId,
}
