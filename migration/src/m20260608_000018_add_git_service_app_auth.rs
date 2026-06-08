use sea_orm_migration::prelude::*;

/// Groundwork for GitHub App (#9) and GitLab OAuth application (#10) auth.
/// Adds an `auth_kind` discriminator (defaulting every existing row to the
/// current `pat` behavior) plus the credential columns each flow will need.
/// No code mints app tokens yet — see `provider::credentials::resolve_token`.
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
                    // GitHub App ID / GitLab OAuth application (client) ID.
                    .add_column(ColumnDef::new(GitServices::AppId).text().null())
                    // GitHub App installation ID (null for GitLab).
                    .add_column(ColumnDef::new(GitServices::AppInstallationId).text().null())
                    // GitHub App private key, PEM (null for GitLab).
                    .add_column(ColumnDef::new(GitServices::AppPrivateKey).text().null())
                    // GitLab OAuth application client secret (null for GitHub).
                    .add_column(ColumnDef::new(GitServices::AppClientSecret).text().null())
                    // GitLab OAuth refresh token (null for GitHub).
                    .add_column(ColumnDef::new(GitServices::AppRefreshToken).text().null())
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
                    .drop_column(GitServices::AppId)
                    .drop_column(GitServices::AppInstallationId)
                    .drop_column(GitServices::AppPrivateKey)
                    .drop_column(GitServices::AppClientSecret)
                    .drop_column(GitServices::AppRefreshToken)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
pub enum GitServices {
    Table,
    AuthKind,
    AppId,
    AppInstallationId,
    AppPrivateKey,
    AppClientSecret,
    AppRefreshToken,
}
