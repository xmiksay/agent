use sea_orm_migration::prelude::*;

/// Groundwork for GitHub App (#9) and GitLab OAuth application (#10) auth.
/// Models the credential as a **type + value** pair: `auth_kind` discriminates
/// the flow (defaulting every existing row to today's `pat`), and the per-flow
/// secrets live in one `app_credentials` JSON blob — so a new provider's app
/// shape needs no schema change. No code mints app tokens yet — see
/// `provider::credentials::resolve_token`.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(GitServices::Table)
                    .add_column(
                        ColumnDef::new(GitServices::AuthKind)
                            .string_len(16)
                            .not_null()
                            .default("pat"),
                    )
                    .add_column(
                        ColumnDef::new(GitServices::AppCredentials)
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
                    .table(GitServices::Table)
                    .drop_column(GitServices::AuthKind)
                    .drop_column(GitServices::AppCredentials)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
pub enum GitServices {
    Table,
    AuthKind,
    AppCredentials,
}
