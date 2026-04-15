use std::env;

use anyhow::{Context, Result};

#[derive(Clone)]
pub struct Config {
    pub webhook_secret: String,
    pub gitlab_token: String,
    pub gitlab_username: String,
    pub gitlab_url: String,
    pub database_url: String,
    pub repo_base_path: String,
    pub max_concurrent_jobs: usize,
    pub job_timeout_secs: u64,
    pub listen_addr: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Self {
            webhook_secret: env::var("WEBHOOK_SECRET")
                .context("WEBHOOK_SECRET must be set")?,
            gitlab_token: env::var("GITLAB_TOKEN")
                .context("GITLAB_TOKEN must be set")?,
            gitlab_username: env::var("MY_GITLAB_USERNAME")
                .context("MY_GITLAB_USERNAME must be set")?,
            gitlab_url: env::var("GITLAB_URL")
                .unwrap_or_else(|_| "https://gitlab.com".to_string()),
            database_url: env::var("DATABASE_URL")
                .context("DATABASE_URL must be set")?,
            repo_base_path: env::var("REPO_BASE_PATH")
                .unwrap_or_else(|_| "/tmp/claude-jobs".to_string()),
            max_concurrent_jobs: env::var("MAX_CONCURRENT_JOBS")
                .unwrap_or_else(|_| "3".to_string())
                .parse()
                .context("MAX_CONCURRENT_JOBS must be a number")?,
            job_timeout_secs: env::var("JOB_TIMEOUT_SECS")
                .unwrap_or_else(|_| "600".to_string())
                .parse()
                .context("JOB_TIMEOUT_SECS must be a number")?,
            listen_addr: env::var("LISTEN_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:3000".to_string()),
        })
    }
}
