use std::env;

use anyhow::{Context, Result};

#[derive(Clone)]
pub struct Config {
    pub api_bearer_token: Option<String>,
    pub database_url: String,
    pub repo_base_path: String,
    pub max_concurrent_jobs: usize,
    pub listen_addr: String,
    /// Externally reachable base URL of this agent (e.g. `https://agent.example.com`),
    /// used to build the callback URL when auto-registering provider webhooks. When
    /// unset, auto-registration is skipped (operators wire hooks by hand).
    pub public_base_url: Option<String>,
    /// Per-task soft budget on output tokens. The runner aborts claude when
    /// cumulative output_tokens reaches 50% of this number (so half is the
    /// safety margin for the active 5h window). Aborted tasks finish as
    /// `failed`/`failed` with the reason noted in task_sessions and session_id
    /// preserved → operator can Resume after reset.
    pub task_token_budget: u64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Self {
            api_bearer_token: env::var("API_BEARER_TOKEN").ok(),
            database_url: env::var("DATABASE_URL").context("DATABASE_URL must be set")?,
            repo_base_path: env::var("REPO_BASE_PATH")
                .unwrap_or_else(|_| "/tmp/claude-jobs".to_string()),
            max_concurrent_jobs: env::var("MAX_CONCURRENT_JOBS")
                .unwrap_or_else(|_| "3".to_string())
                .parse()
                .context("MAX_CONCURRENT_JOBS must be a number")?,
            listen_addr: env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string()),
            public_base_url: env::var("PUBLIC_BASE_URL")
                .ok()
                .map(|v| v.trim_end_matches('/').to_string())
                .filter(|v| !v.is_empty()),
            task_token_budget: env::var("TASK_TOKEN_BUDGET")
                .unwrap_or_else(|_| "1000000".to_string())
                .parse()
                .context("TASK_TOKEN_BUDGET must be a number")?,
        })
    }

    /// Log the resolved config at startup with secrets redacted, so an operator
    /// can confirm e.g. `PUBLIC_BASE_URL` at a glance. Never logs the DB password
    /// or the API token verbatim.
    pub fn log_summary(&self) {
        tracing::info!(
            listen_addr = %self.listen_addr,
            public_base_url = self
                .public_base_url
                .as_deref()
                .unwrap_or("(unset — webhook auto-registration disabled)"),
            repo_base_path = %self.repo_base_path,
            max_concurrent_jobs = self.max_concurrent_jobs,
            task_token_budget = self.task_token_budget,
            api_bearer_token = if self.api_bearer_token.is_some() {
                "set"
            } else {
                "unset (/api is OPEN)"
            },
            database_url = %redact_db_url(&self.database_url),
            "resolved config",
        );
    }
}

/// Mask the password in a Postgres DSN (`scheme://user:pass@host/db` →
/// `scheme://user:***@host/db`) so it's safe to log.
fn redact_db_url(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let Some(at) = url[scheme_end + 3..].find('@').map(|i| i + scheme_end + 3) else {
        return url.to_string();
    };
    let creds = &url[scheme_end + 3..at];
    match creds.find(':') {
        Some(colon) => format!(
            "{}://{}:***@{}",
            &url[..scheme_end],
            &creds[..colon],
            &url[at + 1..]
        ),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::redact_db_url;

    #[test]
    fn redacts_password_keeps_user_and_host() {
        assert_eq!(
            redact_db_url("postgres://bob:s3cr3t@db.host:5432/agent"),
            "postgres://bob:***@db.host:5432/agent"
        );
    }

    #[test]
    fn leaves_passwordless_url_untouched() {
        assert_eq!(
            redact_db_url("postgres://db.host/agent"),
            "postgres://db.host/agent"
        );
    }
}
