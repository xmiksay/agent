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
    /// Seconds the operator has to resolve a tool-approval request before the
    /// runner auto-denies it. `0` (the default) means **wait indefinitely** — the
    /// agent blocks until the operator resolves, never auto-denying.
    pub operator_approval_timeout_secs: u64,
    /// Seconds a single turn may run before the runner SIGKILLs the agent **and
    /// its whole process group** (so orphaned grandchildren like a backgrounded
    /// `cargo test` die too), then finalizes the task resumable (session_id
    /// kept). `0` (the default) disables the watchdog: turns run unbounded.
    pub job_timeout_secs: u64,
    /// NVM install dir (e.g. `/home/agent/.nvm`). When set, the runner resolves
    /// the node toolchain NVM would activate (honouring the worktree's `.nvmrc`)
    /// and prepends its `bin` to the agent's `PATH`. Unset → feature disabled.
    pub nvm_dir: Option<String>,
    /// Admin Postgres DSN with `CREATE ROLE` + `CREATE DATABASE` privileges
    /// (e.g. `postgres://admin:pw@localhost:5432/postgres`). When set, the runner
    /// provisions a throwaway user+database per task and injects its DSN into the
    /// agent's env + initial prompt. Unset → feature disabled.
    pub project_db_admin_url: Option<String>,
    /// The `host:port` the *agent* uses to reach the provisioned DB; may differ
    /// from the admin connection in containerized setups. Defaults to the host
    /// parsed from `project_db_admin_url`.
    pub project_db_host_for_agent: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();
        Self::from_lookup(|key| env::var(key).ok())
    }

    /// Build the config from a key→value lookup. Split out of `from_env` so the
    /// parse/validation logic is testable without mutating the process
    /// environment — `env::set_var` here would race the other DB-backed tests in
    /// this binary that read `DATABASE_URL`.
    fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        Ok(Self {
            api_bearer_token: get("API_BEARER_TOKEN"),
            database_url: get("DATABASE_URL").context("DATABASE_URL must be set")?,
            repo_base_path: get("REPO_BASE_PATH").unwrap_or_else(|| "/tmp/claude-jobs".to_string()),
            max_concurrent_jobs: get("MAX_CONCURRENT_JOBS")
                .unwrap_or_else(|| "3".to_string())
                .parse()
                .context("MAX_CONCURRENT_JOBS must be a number")?,
            listen_addr: get("LISTEN_ADDR").unwrap_or_else(|| "0.0.0.0:3000".to_string()),
            public_base_url: get("PUBLIC_BASE_URL")
                .map(|v| v.trim_end_matches('/').to_string())
                .filter(|v| !v.is_empty()),
            task_token_budget: get("TASK_TOKEN_BUDGET")
                .unwrap_or_else(|| "1000000".to_string())
                .parse()
                .context("TASK_TOKEN_BUDGET must be a number")?,
            operator_approval_timeout_secs: parse_u64_or(
                get("OPERATOR_APPROVAL_TIMEOUT_SECS"),
                0,
                "OPERATOR_APPROVAL_TIMEOUT_SECS",
            )?,
            job_timeout_secs: parse_u64_or(get("JOB_TIMEOUT_SECS"), 0, "JOB_TIMEOUT_SECS")?,
            nvm_dir: get("NVM_DIR").filter(|v| !v.trim().is_empty()),
            project_db_admin_url: get("PROJECT_DB_ADMIN_URL").filter(|v| !v.trim().is_empty()),
            project_db_host_for_agent: get("PROJECT_DB_HOST_FOR_AGENT")
                .filter(|v| !v.trim().is_empty()),
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
            operator_approval_timeout_secs = self.operator_approval_timeout_secs,
            job_timeout_secs = self.job_timeout_secs,
            api_bearer_token = if self.api_bearer_token.is_some() {
                "set"
            } else {
                "unset (/api is OPEN)"
            },
            database_url = %redact_db_url(&self.database_url),
            nvm_dir = self.nvm_dir.as_deref().unwrap_or("(unset — disabled)"),
            project_db_admin_url = self
                .project_db_admin_url
                .as_deref()
                .map(redact_db_url)
                .unwrap_or_else(|| "(unset — disabled)".to_string()),
            project_db_host_for_agent = self
                .project_db_host_for_agent
                .as_deref()
                .unwrap_or("(from admin URL)"),
            "resolved config",
        );
    }
}

/// Parse a `u64` env value, falling back to `default` when unset or empty. A
/// non-numeric value is an error (a typo shouldn't silently become the default).
fn parse_u64_or(raw: Option<String>, default: u64, key: &str) -> Result<u64> {
    match raw {
        Some(v) if !v.trim().is_empty() => v
            .trim()
            .parse()
            .with_context(|| format!("{key} must be a number")),
        _ => Ok(default),
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
    use super::{Config, parse_u64_or, redact_db_url};
    use std::collections::HashMap;

    /// A `get` closure backed by a fixed map, standing in for `env::var`.
    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn from_lookup_happy_parse_fills_defaults() {
        let cfg = Config::from_lookup(lookup(&[(
            "DATABASE_URL",
            "postgres://u:p@localhost/agent",
        )]))
        .expect("happy parse");
        assert_eq!(cfg.database_url, "postgres://u:p@localhost/agent");
        assert_eq!(cfg.repo_base_path, "/tmp/claude-jobs");
        assert_eq!(cfg.max_concurrent_jobs, 3);
        assert_eq!(cfg.listen_addr, "0.0.0.0:3000");
        assert_eq!(cfg.task_token_budget, 1_000_000);
        assert_eq!(cfg.operator_approval_timeout_secs, 0);
        assert_eq!(cfg.job_timeout_secs, 0);
        assert!(cfg.public_base_url.is_none());
        assert!(cfg.api_bearer_token.is_none());
        assert!(cfg.nvm_dir.is_none());
    }

    #[test]
    fn from_lookup_fails_when_database_url_missing() {
        let err = Config::from_lookup(lookup(&[]))
            .err()
            .expect("missing DATABASE_URL must error");
        assert!(err.to_string().contains("DATABASE_URL"), "{err}");
    }

    #[test]
    fn from_lookup_applies_overrides_and_trims_base_url() {
        let cfg = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://localhost/agent"),
            ("MAX_CONCURRENT_JOBS", "8"),
            ("JOB_TIMEOUT_SECS", "1800"),
            // trailing slash trimmed; an empty value collapses to None.
            ("PUBLIC_BASE_URL", "https://agent.example.com/"),
            ("NVM_DIR", "   "),
        ]))
        .expect("override parse");
        assert_eq!(cfg.max_concurrent_jobs, 8);
        assert_eq!(cfg.job_timeout_secs, 1800);
        assert_eq!(
            cfg.public_base_url.as_deref(),
            Some("https://agent.example.com")
        );
        assert!(cfg.nvm_dir.is_none(), "blank NVM_DIR collapses to None");
    }

    #[test]
    fn from_lookup_rejects_non_numeric_budget() {
        let err = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://localhost/agent"),
            ("TASK_TOKEN_BUDGET", "lots"),
        ]))
        .err()
        .expect("non-numeric budget must error");
        assert!(err.to_string().contains("TASK_TOKEN_BUDGET"), "{err}");
    }

    #[test]
    fn approval_timeout_defaults_to_zero_when_unset_or_empty() {
        // 0 is the "wait indefinitely" sentinel — the default operator behavior.
        assert_eq!(
            parse_u64_or(None, 0, "OPERATOR_APPROVAL_TIMEOUT_SECS").unwrap(),
            0
        );
        assert_eq!(
            parse_u64_or(Some("   ".into()), 0, "OPERATOR_APPROVAL_TIMEOUT_SECS").unwrap(),
            0
        );
    }

    #[test]
    fn approval_timeout_parses_explicit_value() {
        assert_eq!(
            parse_u64_or(Some("300".into()), 0, "OPERATOR_APPROVAL_TIMEOUT_SECS").unwrap(),
            300
        );
    }

    #[test]
    fn approval_timeout_rejects_non_numeric() {
        assert!(parse_u64_or(Some("soon".into()), 0, "OPERATOR_APPROVAL_TIMEOUT_SECS").is_err());
    }

    #[test]
    fn job_timeout_defaults_to_zero_when_unset_or_empty() {
        // 0 disables the per-turn watchdog (turns run unbounded) — the default.
        assert_eq!(parse_u64_or(None, 0, "JOB_TIMEOUT_SECS").unwrap(), 0);
        assert_eq!(
            parse_u64_or(Some("  ".into()), 0, "JOB_TIMEOUT_SECS").unwrap(),
            0
        );
    }

    #[test]
    fn job_timeout_parses_explicit_value() {
        assert_eq!(
            parse_u64_or(Some("1800".into()), 0, "JOB_TIMEOUT_SECS").unwrap(),
            1800
        );
    }

    #[test]
    fn job_timeout_rejects_non_numeric() {
        assert!(parse_u64_or(Some("later".into()), 0, "JOB_TIMEOUT_SECS").is_err());
    }

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
