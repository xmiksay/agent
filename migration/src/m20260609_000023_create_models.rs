use sea_orm_migration::prelude::*;

/// The **model catalog**. Three tables plus per-task selection:
///
/// * `model_providers` — the agent backends that can run a model. `kind` is the
///   system-defined key the code resolves to a CLI (`claude_code` today, seeded
///   here); `api_key` is optional and only set when the provider should run in
///   API mode rather than on a subscription login.
/// * `models` — a runnable model: its owning `provider_id`, the `model_id`
///   handed to that provider's CLI, a human `alias`, a price table (per **1M**
///   tokens), and optional `thinking`/`effort` settings. One row may be the
///   global default.
/// * `service_models` — per-service, per-**trigger type** model selection (a
///   service can run `review_mr` on one model and `issue_comment` on another).
/// * `tasks.model_id` — the model a task actually runs, seeded from the service
///   mapping at creation and operator-overridable while pending.
///
/// All FKs degrade safely: deleting a model nulls a task's pick (falls back to
/// the global default at run time) and removes any service mapping; a provider
/// with models cannot be deleted (`RESTRICT`).
#[derive(DeriveMigrationName)]
pub struct Migration;

/// Stable id for the seeded Claude Code provider, so fresh installs always have
/// one runnable backend without an operator setup step.
const CLAUDE_CODE_PROVIDER_ID: &str = "00000000-0000-0000-0000-0000000c0de0";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ModelProviders::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ModelProviders::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ModelProviders::Kind)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(ColumnDef::new(ModelProviders::Name).text().not_null())
                    .col(ColumnDef::new(ModelProviders::ApiKey).text().null())
                    .col(
                        ColumnDef::new(ModelProviders::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ModelProviders::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Seed the one system-defined backend so a fresh install can run.
        let seed = sea_orm_migration::sea_orm::Statement::from_string(
            sea_orm_migration::sea_orm::DatabaseBackend::Postgres,
            format!(
                "INSERT INTO model_providers (id, kind, name, created_at, updated_at) \
                 VALUES ('{CLAUDE_CODE_PROVIDER_ID}', 'claude_code', 'Claude Code', now(), now())"
            ),
        );
        manager.get_connection().execute(seed).await?;

        manager
            .create_table(
                Table::create()
                    .table(Models::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Models::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Models::ProviderId).uuid().not_null())
                    .col(ColumnDef::new(Models::ModelId).text().not_null())
                    .col(ColumnDef::new(Models::Alias).text().not_null())
                    .col(
                        ColumnDef::new(Models::InputPrice)
                            .double()
                            .not_null()
                            .default(0.0),
                    )
                    .col(
                        ColumnDef::new(Models::OutputPrice)
                            .double()
                            .not_null()
                            .default(0.0),
                    )
                    .col(
                        ColumnDef::new(Models::CacheWritePrice)
                            .double()
                            .not_null()
                            .default(0.0),
                    )
                    .col(
                        ColumnDef::new(Models::CacheReadPrice)
                            .double()
                            .not_null()
                            .default(0.0),
                    )
                    .col(
                        ColumnDef::new(Models::Thinking)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(Models::Effort).text().null())
                    .col(
                        ColumnDef::new(Models::IsDefault)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Models::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Models::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_models_provider_id")
                            .from(Models::Table, Models::ProviderId)
                            .to(ModelProviders::Table, ModelProviders::Id)
                            .on_delete(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await?;

        // A model is identified to its CLI by (provider, model_id) — keep unique.
        manager
            .create_index(
                Index::create()
                    .name("idx_models_provider_model_id")
                    .table(Models::Table)
                    .col(Models::ProviderId)
                    .col(Models::ModelId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // At most one global default model.
        let stmt = sea_orm_migration::sea_orm::Statement::from_string(
            sea_orm_migration::sea_orm::DatabaseBackend::Postgres,
            "CREATE UNIQUE INDEX idx_models_one_default \
             ON models (is_default) WHERE is_default = true"
                .to_string(),
        );
        manager.get_connection().execute(stmt).await?;

        manager
            .create_table(
                Table::create()
                    .table(ServiceModels::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ServiceModels::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ServiceModels::ServiceId).uuid().not_null())
                    .col(
                        ColumnDef::new(ServiceModels::TriggerType)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(ColumnDef::new(ServiceModels::ModelId).uuid().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_service_models_service_id")
                            .from(ServiceModels::Table, ServiceModels::ServiceId)
                            .to(Service::Table, Service::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_service_models_model_id")
                            .from(ServiceModels::Table, ServiceModels::ModelId)
                            .to(Models::Table, Models::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_service_models_service_trigger")
                    .table(ServiceModels::Table)
                    .col(ServiceModels::ServiceId)
                    .col(ServiceModels::TriggerType)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .add_column(ColumnDef::new(Tasks::ModelId).uuid().null())
                    .add_foreign_key(
                        TableForeignKey::new()
                            .name("fk_tasks_model_id")
                            .from_tbl(Tasks::Table)
                            .from_col(Tasks::ModelId)
                            .to_tbl(Models::Table)
                            .to_col(Models::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Tasks::Table)
                    .drop_column(Tasks::ModelId)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(ServiceModels::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Models::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ModelProviders::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ModelProviders {
    Table,
    Id,
    Kind,
    Name,
    ApiKey,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Models {
    Table,
    Id,
    ProviderId,
    ModelId,
    Alias,
    InputPrice,
    OutputPrice,
    CacheWritePrice,
    CacheReadPrice,
    Thinking,
    Effort,
    IsDefault,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ServiceModels {
    Table,
    Id,
    ServiceId,
    TriggerType,
    ModelId,
}

#[derive(DeriveIden)]
enum Service {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Tasks {
    Table,
    ModelId,
}
