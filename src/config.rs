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
    /// `failed`/`failed` with the reason noted in task_results and session_id
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
}
