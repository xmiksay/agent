use std::collections::BTreeMap;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::service;
use crate::project::ProviderKind;

/// How a `service` authenticates against its provider.
///
/// `Pat` covers both GitHub/GitLab personal access tokens and GitLab
/// **Group/Project Access Tokens** (the agent's independent bot identity, #10) —
/// the token is used directly as the bearer. `App` is the groundwork for GitHub
/// App (#9) integration — its `app_credentials` bundle is stored and validated,
/// but token minting is not implemented yet (see
/// `crate::provider::credentials::resolve_token`). GitLab has no `app` flow.
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

/// How an issue event triggers the agent for a `service`.
///
/// `Assignee` (today's behavior, the default) fires when the bot is among the
/// issue's assignees. `Label` fires when the issue carries `trigger_label` —
/// this is how a GitHub App, which can't be an issue assignee, gets triggered.
/// `Both` fires on either match.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TriggerMode {
    #[default]
    Assignee,
    Label,
    Both,
}

impl TriggerMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerMode::Assignee => "assignee",
            TriggerMode::Label => "label",
            TriggerMode::Both => "both",
        }
    }
}

impl FromStr for TriggerMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "assignee" => Ok(TriggerMode::Assignee),
            "label" => Ok(TriggerMode::Label),
            "both" => Ok(TriggerMode::Both),
            other => Err(anyhow!("unknown trigger_mode: {other}")),
        }
    }
}

/// The resolved credential shape for a service. Built from `auth_kind` (the
/// type) + `app_credentials` (the value). Consumed by
/// `provider::credentials::resolve_token` (REST calls + the
/// `GH_TOKEN`/`GITLAB_TOKEN` env the agent inherits).
#[derive(Clone, Debug)]
pub enum ServiceCredentials {
    /// Personal/group access token used directly as the bearer. A GitLab
    /// Group/Project Access Token (the agent's bot identity, #10) lands here.
    Pat(String),
    /// GitHub App (#9). JWT → installation-token exchange not implemented yet.
    GitHubApp(GitHubAppConfig),
}

/// `app_credentials` value when `auth_kind = 'app'` and `kind = 'github'`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GitHubAppConfig {
    pub app_id: String,
    pub private_key: String,
    /// Empty until the operator installs the App (the `/github_app/callback`
    /// flow writes it back). `resolve_token` bails with a "not installed yet"
    /// message while it is blank, so a service can be configured pre-install.
    #[serde(default)]
    pub installation_id: String,
    /// REST API base of the owning service (`https://api.github.com` or a GHES
    /// `…/api/v3`). Not part of the stored JSON — threaded in from the service's
    /// `base_url` so token minting hits the right host and the cache keys by it.
    #[serde(skip)]
    pub api_base: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Service {
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
    /// The provider-specific app secret bundle (see `GitHubAppConfig`). `None`
    /// for `pat`. Never serialized to clients.
    #[serde(skip_serializing)]
    pub app_credentials: Option<serde_json::Value>,
    /// How issue events trigger the agent: by assignee, by label, or both.
    pub trigger_mode: TriggerMode,
    /// Label name watched when `trigger_mode` includes labels. Empty = unset.
    pub trigger_label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Service {
    fn from_model(m: service::Model) -> Result<Self> {
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
            app_credentials: m.app_credentials,
            trigger_mode: m.trigger_mode.parse()?,
            trigger_label: m.trigger_label,
            created_at: m.created_at.into(),
            updated_at: m.updated_at.into(),
        })
    }

    /// Resolve the stored config into a typed credential. Fails when an `app`
    /// service is missing (or has a malformed) `app_credentials` value.
    pub fn credentials(&self) -> Result<ServiceCredentials> {
        build_credentials(
            self.kind,
            self.auth_kind,
            &self.token,
            &self.base_url,
            self.app_credentials.as_ref(),
        )
    }
}

/// Single source of truth for turning the type+value columns into a
/// `ServiceCredentials` (and, by extension, for validating that an `app` service
/// has a well-formed `app_credentials` — create/update call this and discard the
/// value).
fn build_credentials(
    kind: ProviderKind,
    auth_kind: AuthKind,
    token: &str,
    base_url: &str,
    app_credentials: Option<&serde_json::Value>,
) -> Result<ServiceCredentials> {
    match auth_kind {
        AuthKind::Pat => Ok(ServiceCredentials::Pat(token.to_string())),
        AuthKind::App => {
            let raw = app_credentials
                .ok_or_else(|| anyhow!("auth_kind 'app' requires app_credentials"))?;
            match kind {
                ProviderKind::Github => {
                    let mut cfg: GitHubAppConfig = serde_json::from_value(raw.clone())
                        .map_err(|e| anyhow!("invalid github app_credentials: {e}"))?;
                    require_nonempty(&cfg.app_id, "app_id")?;
                    require_nonempty(&cfg.private_key, "private_key")?;
                    cfg.api_base = base_url.trim_end_matches('/').to_string();
                    Ok(ServiceCredentials::GitHubApp(cfg))
                }
                ProviderKind::Gitlab => {
                    // GitLab authenticates only via `pat` — a Group/Project Access
                    // Token gives the agent its own bot identity (#10). There is no
                    // GitLab app flow; the OAuth variant was deliberately dropped.
                    let _ = raw;
                    bail!(
                        "gitlab does not support auth_kind 'app'; use a Group/Project Access Token with auth_kind 'pat'"
                    )
                }
            }
        }
    }
}

fn require_nonempty(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("app_credentials.{name} must not be empty");
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewService {
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
    /// Provider-specific app secret bundle; required when `auth_kind = 'app'`.
    #[serde(default)]
    pub app_credentials: Option<serde_json::Value>,
    #[serde(default)]
    pub trigger_mode: TriggerMode,
    #[serde(default)]
    pub trigger_label: String,
    /// Per-trigger-type model mapping to seed (`trigger_type → model_id`). Absent
    /// → no mappings.
    #[serde(default)]
    pub models: Option<BTreeMap<String, Uuid>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct UpdateService {
    pub display_name: Option<String>,
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub webhook_secret: Option<String>,
    pub bot_username: Option<String>,
    pub autofire: Option<bool>,
    pub auth_kind: Option<AuthKind>,
    pub app_credentials: Option<serde_json::Value>,
    pub trigger_mode: Option<TriggerMode>,
    pub trigger_label: Option<String>,
    /// Absent → leave the per-trigger-type model mapping unchanged; present →
    /// replace it wholesale with this `trigger_type → model_id` map (empty clears).
    pub models: Option<BTreeMap<String, Uuid>>,
}

/// Distinguish an absent field (`None`) from an explicit `null` (`Some(None)`)
/// for `Option<Option<T>>` patch fields. Without this, serde collapses both to
/// `None` and a clear-to-null is indistinguishable from "leave unchanged".
pub(crate) fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

#[derive(Clone)]
pub struct ServiceStore {
    db: DatabaseConnection,
}

impl ServiceStore {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<Service>> {
        let rows = service::Entity::find()
            .order_by_asc(service::Column::Kind)
            .order_by_asc(service::Column::Slug)
            .all(&self.db)
            .await?;
        rows.into_iter().map(Service::from_model).collect()
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<Service>> {
        let row = service::Entity::find_by_id(id).one(&self.db).await?;
        row.map(Service::from_model).transpose()
    }

    pub async fn get_by_slug(&self, kind: ProviderKind, slug: &str) -> Result<Option<Service>> {
        let row = service::Entity::find()
            .filter(service::Column::Kind.eq(kind.as_str()))
            .filter(service::Column::Slug.eq(slug))
            .one(&self.db)
            .await?;
        row.map(Service::from_model).transpose()
    }

    pub async fn create(&self, new: NewService) -> Result<Service> {
        validate_slug(&new.slug)?;
        // Reject an `app` service whose app_credentials are missing/malformed.
        build_credentials(
            new.kind,
            new.auth_kind,
            &new.token,
            &new.base_url,
            new.app_credentials.as_ref(),
        )?;

        let now: DateTime<Utc> = Utc::now();
        let id = Uuid::new_v4();
        let active = service::ActiveModel {
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
            app_credentials: Set(new.app_credentials),
            trigger_mode: Set(new.trigger_mode.as_str().to_string()),
            trigger_label: Set(new.trigger_label),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        };
        service::Entity::insert(active)
            .exec(&self.db)
            .await
            .context("failed to insert service")?;

        if let Some(models) = new.models {
            self.set_trigger_models(id, &models).await?;
        }

        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("service disappeared after insert"))
    }

    pub async fn update(&self, id: Uuid, upd: UpdateService) -> Result<Service> {
        let current = self
            .get(id)
            .await?
            .ok_or_else(|| anyhow!("service not found"))?;

        // Resolve the post-patch state (None = keep current) and validate that an
        // `app` service still has well-formed credentials before writing.
        let auth_kind = upd.auth_kind.unwrap_or(current.auth_kind);
        let token = upd.token.clone().unwrap_or_else(|| current.token.clone());
        let app_credentials = upd.app_credentials.clone().or(current.app_credentials);
        let base_url = upd
            .base_url
            .clone()
            .unwrap_or_else(|| current.base_url.clone());
        build_credentials(
            current.kind,
            auth_kind,
            &token,
            &base_url,
            app_credentials.as_ref(),
        )?;

        let mut active: service::ActiveModel = service::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("service not found"))?
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
        if let Some(v) = upd.app_credentials {
            active.app_credentials = Set(Some(v));
        }
        if let Some(v) = upd.trigger_mode {
            active.trigger_mode = Set(v.as_str().to_string());
        }
        if let Some(v) = upd.trigger_label {
            active.trigger_label = Set(v);
        }
        active.updated_at = Set(Utc::now().into());
        active.update(&self.db).await?;

        if let Some(models) = upd.models {
            self.set_trigger_models(id, &models).await?;
        }

        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("service disappeared after update"))
    }

    pub async fn delete(&self, id: Uuid) -> Result<()> {
        let res = service::Entity::delete_by_id(id)
            .exec(&self.db)
            .await
            .context("failed to delete service")?;
        if res.rows_affected == 0 {
            bail!("service not found");
        }
        Ok(())
    }

    pub(crate) fn db(&self) -> &DatabaseConnection {
        &self.db
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
        let new: NewService = serde_json::from_str(json).unwrap();
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
        let new: NewService = serde_json::from_str(json).unwrap();
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
        let new: NewService = serde_json::from_str(json).unwrap();
        assert_eq!(new.auth_kind, AuthKind::Pat);
    }

    #[test]
    fn trigger_mode_default_is_assignee() {
        assert_eq!(TriggerMode::default(), TriggerMode::Assignee);
    }

    #[test]
    fn trigger_mode_roundtrips_through_str() {
        for m in [TriggerMode::Assignee, TriggerMode::Label, TriggerMode::Both] {
            assert_eq!(m.as_str().parse::<TriggerMode>().unwrap(), m);
        }
    }

    #[test]
    fn new_git_service_defaults_trigger_mode_and_label() {
        // Omitting trigger_mode/trigger_label falls back to assignee + empty.
        let json = r#"{
            "kind": "github",
            "slug": "acme",
            "display_name": "Acme",
            "base_url": "https://github.com",
            "token": "t",
            "webhook_secret": "s",
            "bot_username": "bot"
        }"#;
        let new: NewService = serde_json::from_str(json).unwrap();
        assert_eq!(new.trigger_mode, TriggerMode::Assignee);
        assert!(new.trigger_label.is_empty());
    }

    #[test]
    fn new_git_service_parses_explicit_trigger() {
        let json = r#"{
            "kind": "github",
            "slug": "acme",
            "display_name": "Acme",
            "base_url": "https://github.com",
            "token": "t",
            "webhook_secret": "s",
            "bot_username": "bot",
            "trigger_mode": "both",
            "trigger_label": "agent"
        }"#;
        let new: NewService = serde_json::from_str(json).unwrap();
        assert_eq!(new.trigger_mode, TriggerMode::Both);
        assert_eq!(new.trigger_label, "agent");
    }

    #[test]
    fn update_git_service_trigger_fields_default_to_none() {
        let upd: UpdateService = serde_json::from_str("{}").unwrap();
        assert!(upd.trigger_mode.is_none());
        assert!(upd.trigger_label.is_none());
    }

    fn svc(
        kind: ProviderKind,
        auth_kind: AuthKind,
        app_credentials: Option<serde_json::Value>,
    ) -> Service {
        let now = Utc::now();
        Service {
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
            app_credentials,
            trigger_mode: TriggerMode::Assignee,
            trigger_label: String::new(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn credentials_pat_returns_token() {
        let s = svc(ProviderKind::Github, AuthKind::Pat, None);
        match s.credentials().unwrap() {
            ServiceCredentials::Pat(t) => assert_eq!(t, "pat-token"),
            other => panic!("expected Pat, got {other:?}"),
        }
    }

    #[test]
    fn credentials_app_without_credentials_errors() {
        let s = svc(ProviderKind::Github, AuthKind::App, None);
        let err = s.credentials().unwrap_err().to_string();
        assert!(err.contains("app_credentials"), "got: {err}");
    }

    #[test]
    fn credentials_app_github_rejects_missing_field() {
        // Missing `app_id` — a field that is required even pre-install.
        let s = svc(
            ProviderKind::Github,
            AuthKind::App,
            Some(serde_json::json!({ "private_key": "pem", "installation_id": "2" })),
        );
        let err = s.credentials().unwrap_err().to_string();
        assert!(err.contains("app_id"), "got: {err}");
    }

    #[test]
    fn credentials_app_github_allows_blank_installation_id_pre_install() {
        // App config saved before the install flow has run: no installation_id
        // yet. This must resolve (the client can drive the install endpoint); a
        // blank installation_id only fails later, at mint time.
        let s = svc(
            ProviderKind::Github,
            AuthKind::App,
            Some(serde_json::json!({ "app_id": "123", "private_key": "pem" })),
        );
        match s.credentials().unwrap() {
            ServiceCredentials::GitHubApp(cfg) => {
                assert_eq!(cfg.app_id, "123");
                assert!(cfg.installation_id.is_empty());
                assert_eq!(cfg.api_base, "https://example.com");
            }
            other => panic!("expected GitHubApp, got {other:?}"),
        }
    }

    #[test]
    fn credentials_app_github_resolves_when_complete() {
        let s = svc(
            ProviderKind::Github,
            AuthKind::App,
            Some(serde_json::json!({
                "app_id": "123",
                "private_key": "-----BEGIN-----",
                "installation_id": "456",
            })),
        );
        match s.credentials().unwrap() {
            ServiceCredentials::GitHubApp(cfg) => {
                assert_eq!(cfg.app_id, "123");
                assert_eq!(cfg.installation_id, "456");
            }
            other => panic!("expected GitHubApp, got {other:?}"),
        }
    }

    #[test]
    fn credentials_app_gitlab_is_rejected() {
        // GitLab has no `app` flow — its bot identity is a Group/Project Access
        // Token carried through the `pat` path.
        let s = svc(
            ProviderKind::Gitlab,
            AuthKind::App,
            Some(serde_json::json!({ "anything": "here" })),
        );
        let err = s.credentials().unwrap_err().to_string();
        assert!(
            err.contains("does not support auth_kind 'app'"),
            "got: {err}"
        );
    }

    #[test]
    fn credentials_gitlab_pat_returns_token() {
        // A GitLab Group Access Token is just a `pat` bearer.
        let s = svc(ProviderKind::Gitlab, AuthKind::Pat, None);
        match s.credentials().unwrap() {
            ServiceCredentials::Pat(t) => assert_eq!(t, "pat-token"),
            other => panic!("expected Pat, got {other:?}"),
        }
    }
}
