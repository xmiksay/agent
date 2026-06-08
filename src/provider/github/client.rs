use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tracing::info;
use uuid::Uuid;

use crate::project::ProviderKind;
use crate::provider::{BOT_NOTE_MARKER, GitProvider, NoteTarget};

#[derive(Clone)]
pub struct GitHubClient {
    service_id: Uuid,
    client: reqwest::Client,
    /// e.g. https://api.github.com (or a GHES URL).
    api_base: String,
    token: String,
}

impl GitHubClient {
    pub fn new(service_id: Uuid, api_base: &str, token: &str) -> Self {
        Self {
            service_id,
            client: reqwest::Client::builder()
                .user_agent("claude-agent")
                .build()
                .expect("reqwest client"),
            api_base: api_base.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }
}

#[async_trait]
impl GitProvider for GitHubClient {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Github
    }

    fn service_id(&self) -> Uuid {
        self.service_id
    }

    async fn post_note(&self, full_name: &str, target: NoteTarget, body: &str) -> Result<()> {
        let iid = match target {
            NoteTarget::Issue(iid) | NoteTarget::MergeRequest(iid) => iid,
        };
        // GitHub treats PR review comments at /issues/<num>/comments too.
        let url = format!("{}/repos/{full_name}/issues/{iid}/comments", self.api_base,);
        info!(%url, "posting comment to GitHub");
        let stamped = format!("{body}\n\n{BOT_NOTE_MARKER}");
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::json!({ "body": stamped }))
            .send()
            .await
            .context("posting GitHub comment")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("GitHub API error {status}: {text}");
        }
        Ok(())
    }
}
