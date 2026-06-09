//! GitLab Group/Project Access Token provisioning (#10): mint a dedicated bot
//! token from an owner-scoped bootstrap token, then rotate it in place.
//!
//! GitLab has no App install. The agent's independent identity is a Group (or
//! Project) Access Token — a plain `pat` bearer. "Provisioning" mints that token
//! via the access-tokens REST API using the operator's owner token, then the
//! service swaps its stored token for the scoped bot token. Rotation hits the
//! `/rotate` endpoint authenticated with the bot token itself (its `api` scope
//! authorizes self-rotation), so the operator's owner token is needed only once.

use anyhow::{Context, Result, bail};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

/// Maintainer — the minimum role to register project webhooks and push to
/// protected branches (see `docs/application-integration.md`).
const MAINTAINER: i64 = 40;

/// Scopes the agent needs end to end: `api` (notes, MRs, webhook registration,
/// self-rotation) and `write_repository` (git push over token-HTTPS, #22).
const BOT_SCOPES: [&str; 2] = ["api", "write_repository"];

/// GitLab caps access-token lifetime at 365 days; default one day under the cap.
const DEFAULT_LIFETIME_DAYS: i64 = 364;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenScope {
    Group,
    Project,
}

impl TokenScope {
    /// REST collection segment: `/api/v4/{groups|projects}/…`.
    fn segment(self) -> &'static str {
        match self {
            TokenScope::Group => "groups",
            TokenScope::Project => "projects",
        }
    }
}

/// What `provision` is given. `namespace` is a group/project path
/// (`my-group/sub`) or numeric id; `expires_at` is `YYYY-MM-DD`.
#[derive(Clone, Debug)]
pub struct ProvisionParams {
    pub scope: TokenScope,
    pub namespace: String,
    pub name: String,
    pub expires_at: String,
}

/// Persisted alongside the bot token (in the service `app_credentials` bundle)
/// so a later rotation knows which token to rotate and where. Non-secret.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenMeta {
    pub scope: TokenScope,
    pub namespace: String,
    pub token_id: i64,
    pub expires_at: Option<String>,
}

/// The minted secret plus the metadata a rotation needs.
#[derive(Clone, Debug)]
pub struct MintedToken {
    pub token: String,
    pub token_id: i64,
    pub expires_at: Option<String>,
}

#[derive(Deserialize)]
struct AccessTokenResponse {
    id: i64,
    token: String,
    expires_at: Option<String>,
}

/// `YYYY-MM-DD`, `DEFAULT_LIFETIME_DAYS` from today — the default token expiry
/// when the operator doesn't pick one.
pub fn default_expiry() -> String {
    (Utc::now().date_naive() + Duration::days(DEFAULT_LIFETIME_DAYS))
        .format("%Y-%m-%d")
        .to_string()
}

/// Mint a fresh Group/Project Access Token using `bootstrap_token` (must be an
/// Owner for group scope, Maintainer+ for project scope).
pub async fn provision(
    base_url: &str,
    bootstrap_token: &str,
    params: &ProvisionParams,
) -> Result<MintedToken> {
    let base = base_url.trim_end_matches('/');
    let enc = urlencoding::encode(&params.namespace);
    let url = format!(
        "{base}/api/v4/{}/{enc}/access_tokens",
        params.scope.segment()
    );
    let body = serde_json::json!({
        "name": params.name,
        "scopes": BOT_SCOPES,
        "access_level": MAINTAINER,
        "expires_at": params.expires_at,
    });
    let resp = reqwest::Client::new()
        .post(&url)
        .header("PRIVATE-TOKEN", bootstrap_token)
        .json(&body)
        .send()
        .await
        .context("requesting GitLab access token")?;
    parse(resp).await
}

/// Rotate an existing token in place. The current bot token authorizes its own
/// rotation (it carries `api` scope); GitLab revokes the old value and returns a
/// new one (with a new id) under the same expiry window.
pub async fn rotate(
    base_url: &str,
    current_token: &str,
    meta: &TokenMeta,
    expires_at: &str,
) -> Result<MintedToken> {
    let base = base_url.trim_end_matches('/');
    let enc = urlencoding::encode(&meta.namespace);
    let url = format!(
        "{base}/api/v4/{}/{enc}/access_tokens/{}/rotate",
        meta.scope.segment(),
        meta.token_id,
    );
    let resp = reqwest::Client::new()
        .post(&url)
        .header("PRIVATE-TOKEN", current_token)
        .json(&serde_json::json!({ "expires_at": expires_at }))
        .send()
        .await
        .context("rotating GitLab access token")?;
    parse(resp).await
}

async fn parse(resp: reqwest::Response) -> Result<MintedToken> {
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("GitLab access-token API error {status}: {text}");
    }
    let body: AccessTokenResponse = resp.json().await.context("parsing GitLab access token")?;
    if body.token.trim().is_empty() {
        bail!("GitLab returned an empty token");
    }
    Ok(MintedToken {
        token: body.token,
        token_id: body.id,
        expires_at: body.expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_segments() {
        assert_eq!(TokenScope::Group.segment(), "groups");
        assert_eq!(TokenScope::Project.segment(), "projects");
    }

    #[test]
    fn scope_serdes_lowercase() {
        assert_eq!(
            serde_json::to_value(TokenScope::Group).unwrap(),
            serde_json::json!("group")
        );
        let s: TokenScope = serde_json::from_value(serde_json::json!("project")).unwrap();
        assert_eq!(s, TokenScope::Project);
    }

    #[test]
    fn token_meta_round_trips_through_json() {
        let meta = TokenMeta {
            scope: TokenScope::Group,
            namespace: "my-group/sub".into(),
            token_id: 42,
            expires_at: Some("2027-06-08".into()),
        };
        let v = serde_json::to_value(&meta).unwrap();
        let back: TokenMeta = serde_json::from_value(v).unwrap();
        assert_eq!(back.token_id, 42);
        assert_eq!(back.scope, TokenScope::Group);
        assert_eq!(back.namespace, "my-group/sub");
    }

    #[test]
    fn default_expiry_is_iso_date_under_a_year_out() {
        let e = default_expiry();
        // YYYY-MM-DD and parseable.
        let parsed = chrono::NaiveDate::parse_from_str(&e, "%Y-%m-%d").unwrap();
        let days = (parsed - Utc::now().date_naive()).num_days();
        assert!((360..=365).contains(&days), "got {days} days");
    }
}
