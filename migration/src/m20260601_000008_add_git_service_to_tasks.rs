use sea_orm_migration::prelude::*;

use crate::m20260415_000001_create_tasks::Tasks;
use crate::m20260601_000007_create_git_services::GitServices;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .add_column(ColumnDef::new(TasksExt::GitServiceId).uuid().null())
                    .add_foreign_key(
                        TableForeignKey::new()
                            .name("fk_tasks_git_service_id")
                            .from_tbl(Tasks::Table)
                            .from_col(TasksExt::GitServiceId)
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
                    .name("idx_tasks_git_service_id")
                    .table(Tasks::Table)
                    .col(TasksExt::GitServiceId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_tasks_git_service_id")
                    .table(Tasks::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .drop_column(TasksExt::GitServiceId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum TasksExt {
    GitServiceId,
}
