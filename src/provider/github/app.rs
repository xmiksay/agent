//! GitHub App auth: JWT signing, installation-token minting (+ cache), and the
//! install-URL lookup the operator's "Install App" button redirects to.
//!
//! The two-step REST flow (see `docs/application-integration.md`): sign a
//! short-lived RS256 JWT with the App's private key (`iss = app_id`), then
//! exchange it at `POST /app/installations/{id}/access_tokens` for an
//! installation access token (~1h TTL) scoped to that installation. The token is
//! cached in-process until ~5 min before expiry so the per-request `resolve_token`
//! callers don't mint one every time.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;

use crate::service::GitHubAppConfig;

/// Refresh this long before the stated `expires_at` so a token never goes stale
/// mid-request.
const REFRESH_MARGIN: Duration = Duration::minutes(5);

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent("claude-agent")
        .build()
        .expect("reqwest client")
});

#[derive(Clone)]
struct CachedToken {
    token: String,
    expires_at: DateTime<Utc>,
}

/// Keyed by `{api_base}#{installation_id}` — uniquely identifies an installation
/// regardless of how many services point at it.
static CACHE: LazyLock<Mutex<HashMap<String, CachedToken>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn cache_key(cfg: &GitHubAppConfig) -> String {
    format!("{}#{}", cfg.api_base, cfg.installation_id)
}

/// Resolve a usable installation access token for this App config, minting and
/// caching one when the cached entry is missing or within `REFRESH_MARGIN` of
/// expiry. This is the entry point `resolve_token`'s `GitHubApp` arm calls.
pub async fn installation_token(cfg: &GitHubAppConfig) -> Result<String> {
    if cfg.installation_id.trim().is_empty() {
        bail!("GitHub App is not installed yet — run the install flow so installation_id is set");
    }

    let key = cache_key(cfg);
    if let Some(hit) = cache_lookup(&key) {
        return Ok(hit);
    }

    let (token, expires_at) = mint_installation_token(cfg).await?;
    cache_store(key, &token, expires_at);
    Ok(token)
}

fn cache_lookup(key: &str) -> Option<String> {
    let guard = CACHE.lock().expect("token cache mutex");
    let entry = guard.get(key)?;
    (Utc::now() < entry.expires_at - REFRESH_MARGIN).then(|| entry.token.clone())
}

fn cache_store(key: String, token: &str, expires_at: DateTime<Utc>) {
    let mut guard = CACHE.lock().expect("token cache mutex");
    guard.insert(
        key,
        CachedToken {
            token: token.to_string(),
            expires_at,
        },
    );
}

#[derive(Serialize)]
struct Claims {
    iat: i64,
    exp: i64,
    iss: String,
}

/// Sign a short-lived app-level JWT (RS256). `iat` is backdated 60s to tolerate
/// clock skew and `exp` is well under GitHub's 10-minute ceiling.
fn mint_jwt(cfg: &GitHubAppConfig) -> Result<String> {
    let now = Utc::now().timestamp();
    let claims = Claims {
        iat: now - 60,
        exp: now + 9 * 60,
        iss: cfg.app_id.clone(),
    };
    let key = EncodingKey::from_rsa_pem(cfg.private_key.as_bytes())
        .context("parsing GitHub App private key (expected an RSA PEM)")?;
    encode(&Header::new(Algorithm::RS256), &claims, &key).context("signing GitHub App JWT")
}

async fn mint_installation_token(cfg: &GitHubAppConfig) -> Result<(String, DateTime<Utc>)> {
    let jwt = mint_jwt(cfg)?;
    let url = format!(
        "{}/app/installations/{}/access_tokens",
        cfg.api_base, cfg.installation_id
    );
    let resp = CLIENT
        .post(&url)
        .bearer_auth(&jwt)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .context("requesting GitHub installation token")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("GitHub installation-token error {status}: {text}");
    }
    let body: InstallationTokenResponse =
        resp.json().await.context("parsing installation token")?;
    Ok((body.token, body.expires_at))
}

#[derive(serde::Deserialize)]
struct InstallationTokenResponse {
    token: String,
    expires_at: DateTime<Utc>,
}

/// The URL the operator is redirected to in order to install the App, with
/// `state` round-tripped back to our callback. Looks up the App's public
/// `html_url` via `GET /app` (a JWT-authed call) so it works for github.com and
/// GHES alike.
pub async fn install_url(cfg: &GitHubAppConfig, state: &str) -> Result<String> {
    let jwt = mint_jwt(cfg)?;
    let url = format!("{}/app", cfg.api_base);
    let resp = CLIENT
        .get(&url)
        .bearer_auth(&jwt)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .context("looking up GitHub App (GET /app)")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("GitHub GET /app error {status}: {text}");
    }
    let app: AppResponse = resp.json().await.context("parsing GET /app")?;
    Ok(format!(
        "{}/installations/new?state={}",
        app.html_url.trim_end_matches('/'),
        urlencoding::encode(state)
    ))
}

#[derive(serde::Deserialize)]
struct AppResponse {
    html_url: String,
}

/// Discover the App's installation id via `GET /app/installations` (JWT-authed),
/// so we don't depend on the post-install redirect carrying it back. Returns the
/// first installation; errors if the App isn't installed anywhere yet.
pub async fn discover_installation_id(cfg: &GitHubAppConfig) -> Result<String> {
    let jwt = mint_jwt(cfg)?;
    let url = format!("{}/app/installations", cfg.api_base);
    let resp = CLIENT
        .get(&url)
        .bearer_auth(&jwt)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .context("listing GitHub App installations")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("GitHub GET /app/installations error {status}: {text}");
    }
    let installs: Vec<InstallationItem> = resp.json().await.context("parsing installations")?;
    let Some(first) = installs.first() else {
        bail!("the App is not installed on any account yet — install it on GitHub first");
    };
    Ok(first.id.to_string())
}

#[derive(serde::Deserialize)]
struct InstallationItem {
    id: u64,
}

/// Point the App's single app-level webhook at `webhook_url` with `secret`, via
/// `PATCH /app/hook/config` (JWT-authed). Lets the agent register its own inbound
/// webhook instead of the operator wiring it in the App settings by hand. Note:
/// event subscriptions and the webhook "active" toggle are part of the App's
/// definition and are not settable here — only url/secret/content_type.
pub async fn set_app_webhook(cfg: &GitHubAppConfig, webhook_url: &str, secret: &str) -> Result<()> {
    let jwt = mint_jwt(cfg)?;
    let url = format!("{}/app/hook/config", cfg.api_base);
    let resp = CLIENT
        .patch(&url)
        .bearer_auth(&jwt)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&serde_json::json!({
            "url": webhook_url,
            "secret": secret,
            "content_type": "json",
            "insecure_ssl": "0",
        }))
        .send()
        .await
        .context("setting GitHub App webhook config")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("GitHub PATCH /app/hook/config error {status}: {text}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(installation_id: &str) -> GitHubAppConfig {
        GitHubAppConfig {
            app_id: "1".into(),
            // A throwaway 2048-bit RSA key would bloat the test; signing is
            // exercised indirectly. Here we only assert the pre-install guard.
            private_key: String::new(),
            installation_id: installation_id.into(),
            api_base: "https://api.github.com".into(),
        }
    }

    #[tokio::test]
    async fn blank_installation_id_is_a_clear_error() {
        let err = installation_token(&cfg("  "))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not installed"), "got: {err}");
    }
}
