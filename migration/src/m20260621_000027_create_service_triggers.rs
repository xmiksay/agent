use sea_orm_migration::prelude::*;

/// Per-trigger-type gating overrides for a service (`service_triggers`). One row
/// per overridden trigger type (`issue`/`review_mr`/`fix_review`/`mr_comment`/
/// `issue_comment`) carrying `enabled` (default true), `mode` (default
/// `assignee`), and `label` (default `''`). A row OVERRIDES the service-level
/// `trigger_mode`/`trigger_label` default for that type; absence means enabled
/// with the service defaults. Unique on `(service_id, trigger_type)`; the FK
/// cascade-deletes with the owning service.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ServiceTriggers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ServiceTriggers::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ServiceTriggers::ServiceId).uuid().not_null())
                    .col(
                        ColumnDef::new(ServiceTriggers::TriggerType)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ServiceTriggers::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(ServiceTriggers::Mode)
                            .string_len(16)
                            .not_null()
                            .default("assignee"),
                    )
                    .col(
                        ColumnDef::new(ServiceTriggers::Label)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_service_triggers_service_id")
                            .from(ServiceTriggers::Table, ServiceTriggers::ServiceId)
                            .to(Service::Table, Service::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_service_triggers_service_type")
                    .table(ServiceTriggers::Table)
                    .col(ServiceTriggers::ServiceId)
                    .col(ServiceTriggers::TriggerType)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ServiceTriggers::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ServiceTriggers {
    Table,
    Id,
    ServiceId,
    TriggerType,
    Enabled,
    Mode,
    Label,
}

#[derive(DeriveIden)]
enum Service {
    Table,
    Id,
}
