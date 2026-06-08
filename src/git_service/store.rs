use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::git_services;
use crate::project::ProviderKind;

/// How a `git_service` authenticates against its provider.
///
/// `Pat` is the only flow wired today. `App` is the groundwork for GitHub App
/// (#9) and GitLab OAuth application (#10) integration — its credential columns
/// are stored and validated, but token minting is not implemented yet (see
/// `crate::provider::credentials::resolve_token`).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    #[default]
    Pat,
    App,
}

impl AuthKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthKind::Pat => "pat",
            AuthKind::App => "app",
        }
    }
}

impl FromStr for AuthKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "pat" => Ok(AuthKind::Pat),
            "app" => Ok(AuthKind::App),
            other => Err(anyhow!("unknown auth_kind: {other}")),
        }
    }
}

/// The resolved credential shape for a service, derived from `auth_kind` + the
/// provider-specific columns. Consumed by `provider::credentials::resolve_token`
/// (REST calls + the `GH_TOKEN`/`GITLAB_TOKEN` env the agent inherits).
#[derive(Clone, Debug)]
pub enum ServiceCredentials {
    /// Personal/group access token used directly as the bearer.
    Pat(String),
    /// GitHub App (#9). JWT → installation-token exchange not implemented yet.
    GitHubApp {
        app_id: String,
        private_key: String,
        installation_id: String,
    },
    /// GitLab OAuth application (#10). Refresh-token exchange not implemented yet.
    GitLabOAuth {
        client_id: String,
        client_secret: String,
        refresh_token: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct GitService {
    pub id: Uuid,
    pub kind: ProviderKind,
    pub slug: String,
    pub display_name: String,
    pub base_url: String,
    pub token: String,
    pub webhook_secret: String,
    pub bot_username: String,
    pub autofire: bool,
    pub auth_kind: AuthKind,
    pub app_id: Option<String>,
    pub app_installation_id: Option<String>,
    pub app_private_key: Option<String>,
    pub app_client_secret: Option<String>,
    pub app_refresh_token: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl GitService {
    fn from_model(m: git_services::Model) -> Result<Self> {
        Ok(Self {
            id: m.id,
            kind: m.kind.parse()?,
            slug: m.slug,
            display_name: m.display_name,
            base_url: m.base_url,
            token: m.token,
            webhook_secret: m.webhook_secret,
            bot_username: m.bot_username,
            autofire: m.autofire,
            auth_kind: m.auth_kind.parse()?,
            app_id: m.app_id,
            app_installation_id: m.app_installation_id,
            app_private_key: m.app_private_key,
            app_client_secret: m.app_client_secret,
            app_refresh_token: m.app_refresh_token,
            created_at: m.created_at.into(),
            updated_at: m.updated_at.into(),
        })
    }

    /// Resolve the stored config into a typed credential. Fails when an `app`
    /// service is missing a column its provider requires.
    pub fn credentials(&self) -> Result<ServiceCredentials> {
        build_credentials(
            self.kind,
            self.auth_kind,
            &self.token,
            &self.app_id,
            &self.app_installation_id,
            &self.app_private_key,
            &self.app_client_secret,
            &self.app_refresh_token,
        )
    }
}

/// Single source of truth for turning stored columns into a `ServiceCredentials`
/// (and, by extension, for validating that an `app` service has the columns its
/// provider needs — create/update call this and discard the value).
#[allow(clippy::too_many_arguments)]
fn build_credentials(
    kind: ProviderKind,
    auth_kind: AuthKind,
    token: &str,
    app_id: &Option<String>,
    app_installation_id: &Option<String>,
    app_private_key: &Option<String>,
    app_client_secret: &Option<String>,
    app_refresh_token: &Option<String>,
) -> Result<ServiceCredentials> {
    match auth_kind {
        AuthKind::Pat => Ok(ServiceCredentials::Pat(token.to_string())),
        AuthKind::App => match kind {
            ProviderKind::Github => Ok(ServiceCredentials::GitHubApp {
                app_id: require_field(app_id, "app_id")?,
                private_key: require_field(app_private_key, "app_private_key")?,
                installation_id: require_field(app_installation_id, "app_installation_id")?,
            }),
            ProviderKind::Gitlab => Ok(ServiceCredentials::GitLabOAuth {
                client_id: require_field(app_id, "app_id")?,
                client_secret: require_field(app_client_secret, "app_client_secret")?,
                refresh_token: require_field(app_refresh_token, "app_refresh_token")?,
            }),
        },
    }
}

fn require_field(value: &Option<String>, name: &str) -> Result<String> {
    match value {
        Some(v) if !v.trim().is_empty() => Ok(v.clone()),
        _ => bail!("auth_kind 'app' requires {name}"),
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewGitService {
    pub kind: ProviderKind,
    pub slug: String,
    pub display_name: String,
    pub base_url: String,
    #[serde(default)]
    pub token: String,
    pub webhook_secret: String,
    pub bot_username: String,
    #[serde(default)]
    pub autofire: bool,
    #[serde(default)]
    pub auth_kind: AuthKind,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub app_installation_id: Option<String>,
    #[serde(default)]
    pub app_private_key: Option<String>,
    #[serde(default)]
    pub app_client_secret: Option<String>,
    #[serde(default)]
    pub app_refresh_token: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct UpdateGitService {
    pub display_name: Option<String>,
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub webhook_secret: Option<String>,
    pub bot_username: Option<String>,
    pub autofire: Option<bool>,
    pub auth_kind: Option<AuthKind>,
    pub app_id: Option<String>,
    pub app_installation_id: Option<String>,
    pub app_private_key: Option<String>,
    pub app_client_secret: Option<String>,
    pub app_refresh_token: Option<String>,
}

#[derive(Clone)]
pub struct GitServiceStore {
    db: DatabaseConnection,
}

impl GitServiceStore {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<GitService>> {
        let rows = git_services::Entity::find()
            .order_by_asc(git_services::Column::Kind)
            .order_by_asc(git_services::Column::Slug)
            .all(&self.db)
            .await?;
        rows.into_iter().map(GitService::from_model).collect()
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<GitService>> {
        let row = git_services::Entity::find_by_id(id).one(&self.db).await?;
        row.map(GitService::from_model).transpose()
    }

    pub async fn get_by_slug(&self, kind: ProviderKind, slug: &str) -> Result<Option<GitService>> {
        let row = git_services::Entity::find()
            .filter(git_services::Column::Kind.eq(kind.as_str()))
            .filter(git_services::Column::Slug.eq(slug))
            .one(&self.db)
            .await?;
        row.map(GitService::from_model).transpose()
    }

    pub async fn create(&self, new: NewGitService) -> Result<GitService> {
        validate_slug(&new.slug)?;
        // Reject an `app` service that's missing the columns its provider needs.
        build_credentials(
            new.kind,
            new.auth_kind,
            &new.token,
            &new.app_id,
            &new.app_installation_id,
            &new.app_private_key,
            &new.app_client_secret,
            &new.app_refresh_token,
        )?;
        if matches!(new.kind, ProviderKind::Github) {
            let exists = git_services::Entity::find()
                .filter(git_services::Column::Kind.eq(new.kind.as_str()))
                .one(&self.db)
                .await?;
            if exists.is_some() {
                bail!("a github service is already configured");
            }
        }

        let now: DateTime<Utc> = Utc::now();
        let id = Uuid::new_v4();
        let active = git_services::ActiveModel {
            id: Set(id),
            kind: Set(new.kind.as_str().to_string()),
            slug: Set(new.slug),
            display_name: Set(new.display_name),
            base_url: Set(new.base_url.trim_end_matches('/').to_string()),
            token: Set(new.token),
            webhook_secret: Set(new.webhook_secret),
            bot_username: Set(new.bot_username),
            autofire: Set(new.autofire),
            auth_kind: Set(new.auth_kind.as_str().to_string()),
            app_id: Set(new.app_id),
            app_installation_id: Set(new.app_installation_id),
            app_private_key: Set(new.app_private_key),
            app_client_secret: Set(new.app_client_secret),
            app_refresh_token: Set(new.app_refresh_token),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        };
        git_services::Entity::insert(active)
            .exec(&self.db)
            .await
            .context("failed to insert git_service")?;

        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("git_service disappeared after insert"))
    }

    pub async fn update(&self, id: Uuid, upd: UpdateGitService) -> Result<GitService> {
        let current = self
            .get(id)
            .await?
            .ok_or_else(|| anyhow!("git_service not found"))?;

        // Resolve the post-patch state (None = keep current) and validate that an
        // `app` service still has every column its provider needs before writing.
        let auth_kind = upd.auth_kind.unwrap_or(current.auth_kind);
        let token = upd.token.clone().unwrap_or_else(|| current.token.clone());
        let app_id = upd.app_id.clone().or(current.app_id);
        let app_installation_id = upd
            .app_installation_id
            .clone()
            .or(current.app_installation_id);
        let app_private_key = upd.app_private_key.clone().or(current.app_private_key);
        let app_client_secret = upd.app_client_secret.clone().or(current.app_client_secret);
        let app_refresh_token = upd.app_refresh_token.clone().or(current.app_refresh_token);
        build_credentials(
            current.kind,
            auth_kind,
            &token,
            &app_id,
            &app_installation_id,
            &app_private_key,
            &app_client_secret,
            &app_refresh_token,
        )?;

        let mut active: git_services::ActiveModel = git_services::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("git_service not found"))?
            .into();

        if let Some(v) = upd.display_name {
            active.display_name = Set(v);
        }
        if let Some(v) = upd.base_url {
            active.base_url = Set(v.trim_end_matches('/').to_string());
        }
        if let Some(v) = upd.token {
            active.token = Set(v);
        }
        if let Some(v) = upd.webhook_secret {
            active.webhook_secret = Set(v);
        }
        if let Some(v) = upd.bot_username {
            active.bot_username = Set(v);
        }
        if let Some(v) = upd.autofire {
            active.autofire = Set(v);
        }
        if let Some(v) = upd.auth_kind {
            active.auth_kind = Set(v.as_str().to_string());
        }
        if let Some(v) = upd.app_id {
            active.app_id = Set(Some(v));
        }
        if let Some(v) = upd.app_installation_id {
            active.app_installation_id = Set(Some(v));
        }
        if let Some(v) = upd.app_private_key {
            active.app_private_key = Set(Some(v));
        }
        if let Some(v) = upd.app_client_secret {
            active.app_client_secret = Set(Some(v));
        }
        if let Some(v) = upd.app_refresh_token {
            active.app_refresh_token = Set(Some(v));
        }
        active.updated_at = Set(Utc::now().into());
        active.update(&self.db).await?;

        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("git_service disappeared after update"))
    }

    pub async fn delete(&self, id: Uuid) -> Result<()> {
        let res = git_services::Entity::delete_by_id(id)
            .exec(&self.db)
            .await
            .context("failed to delete git_service")?;
        if res.rows_affected == 0 {
            bail!("git_service not found");
        }
        Ok(())
    }
}

fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty() {
        bail!("slug is required");
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("slug must be ASCII alphanumeric, '-' or '_'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_git_service_defaults_autofire_to_false() {
        let json = r#"{
            "kind": "github",
            "slug": "acme",
            "display_name": "Acme",
            "base_url": "https://github.com",
            "token": "t",
            "webhook_secret": "s",
            "bot_username": "bot"
        }"#;
        let new: NewGitService = serde_json::from_str(json).unwrap();
        assert!(!new.autofire);
    }

    #[test]
    fn new_git_service_parses_explicit_autofire() {
        let json = r#"{
            "kind": "gitlab",
            "slug": "acme",
            "display_name": "Acme",
            "base_url": "https://gitlab.com",
            "token": "t",
            "webhook_secret": "s",
            "bot_username": "bot",
            "autofire": true
        }"#;
        let new: NewGitService = serde_json::from_str(json).unwrap();
        assert!(new.autofire);
    }

    #[test]
    fn new_git_service_defaults_auth_kind_to_pat() {
        let json = r#"{
            "kind": "github",
            "slug": "acme",
            "display_name": "Acme",
            "base_url": "https://github.com",
            "token": "t",
            "webhook_secret": "s",
            "bot_username": "bot"
        }"#;
        let new: NewGitService = serde_json::from_str(json).unwrap();
        assert_eq!(new.auth_kind, AuthKind::Pat);
    }

    fn svc(kind: ProviderKind, auth_kind: AuthKind) -> GitService {
        let now = Utc::now();
        GitService {
            id: Uuid::nil(),
            kind,
            slug: "s".into(),
            display_name: "S".into(),
            base_url: "https://example.com".into(),
            token: "pat-token".into(),
            webhook_secret: "wh".into(),
            bot_username: "bot".into(),
            autofire: false,
            auth_kind,
            app_id: None,
            app_installation_id: None,
            app_private_key: None,
            app_client_secret: None,
            app_refresh_token: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn credentials_pat_returns_token() {
        let s = svc(ProviderKind::Github, AuthKind::Pat);
        match s.credentials().unwrap() {
            ServiceCredentials::Pat(t) => assert_eq!(t, "pat-token"),
            other => panic!("expected Pat, got {other:?}"),
        }
    }

    #[test]
    fn credentials_app_github_requires_columns() {
        // No app columns set → error naming the first missing field.
        let s = svc(ProviderKind::Github, AuthKind::App);
        let err = s.credentials().unwrap_err().to_string();
        assert!(err.contains("app_id"), "got: {err}");
    }

    #[test]
    fn credentials_app_github_resolves_when_complete() {
        let mut s = svc(ProviderKind::Github, AuthKind::App);
        s.app_id = Some("123".into());
        s.app_private_key = Some("-----BEGIN-----".into());
        s.app_installation_id = Some("456".into());
        match s.credentials().unwrap() {
            ServiceCredentials::GitHubApp {
                app_id,
                installation_id,
                ..
            } => {
                assert_eq!(app_id, "123");
                assert_eq!(installation_id, "456");
            }
            other => panic!("expected GitHubApp, got {other:?}"),
        }
    }

    #[test]
    fn credentials_app_gitlab_requires_client_secret() {
        let mut s = svc(ProviderKind::Gitlab, AuthKind::App);
        s.app_id = Some("client-id".into());
        // Missing client secret + refresh token.
        let err = s.credentials().unwrap_err().to_string();
        assert!(err.contains("app_client_secret"), "got: {err}");
    }
}
