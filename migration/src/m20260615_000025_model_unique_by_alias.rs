use sea_orm_migration::prelude::*;

/// Re-key model uniqueness from `(provider_id, model_id)` to the user-facing
/// `alias`. The same `model_id` (e.g. `claude-opus-4-8`) may now be registered
/// more than once with different configuration (thinking on/off, unbound,
/// distinct effort/pricing); the `alias` is the operator's unique label and so
/// becomes globally unique across all model rows. The `idx_models_one_default`
/// partial unique index is left untouched.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_models_provider_model_id")
                    .table(Models::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_models_alias")
                    .table(Models::Table)
                    .col(Models::Alias)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_models_alias")
                    .table(Models::Table)
                    .to_owned(),
            )
            .await?;

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
            .await
    }
}

#[derive(DeriveIden)]
enum Models {
    Table,
    ProviderId,
    ModelId,
    Alias,
}
