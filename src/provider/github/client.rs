use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tracing::info;
use uuid::Uuid;

use crate::project::ProviderKind;
use crate::provider::{
    BOT_NOTE_MARKER, GitProvider, NoteTarget, resolve_token, webhook_path_marker,
};
use crate::service::ServiceCredentials;

#[derive(Clone)]
pub struct GitHubClient {
    service_id: Uuid,
    client: reqwest::Client,
    /// e.g. https://api.github.com (or a GHES URL).
    api_base: String,
    creds: ServiceCredentials,
}

impl GitHubClient {
    pub fn new(service_id: Uuid, api_base: &str, creds: ServiceCredentials) -> Self {
        Self {
            service_id,
            client: reqwest::Client::builder()
                .user_agent("claude-agent")
                .build()
                .expect("reqwest client"),
            api_base: api_base.trim_end_matches('/').to_string(),
            creds,
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
        let token = resolve_token(&self.creds).await?;
        let stamped = format!("{body}\n\n{BOT_NOTE_MARKER}");
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&token)
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

    async fn ensure_webhook(&self, repo_path: &str, webhook_url: &str, secret: &str) -> Result<()> {
        let token = resolve_token(&self.creds).await?;
        let hooks_url = format!("{}/repos/{repo_path}/hooks", self.api_base);
        let marker = webhook_path_marker(webhook_url);

        let list = self
            .client
            .get(&hooks_url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .context("listing GitHub hooks")?;
        if !list.status().is_success() {
            let status = list.status();
            let text = list.text().await.unwrap_or_default();
            bail!("GitHub hooks list error {status}: {text}");
        }
        let hooks: Vec<serde_json::Value> = list.json().await.context("parsing GitHub hooks")?;
        let existing_id = hooks.iter().find_map(|h| {
            let url = h.get("config")?.get("url")?.as_str()?;
            url.ends_with(marker)
                .then(|| h.get("id")?.as_i64())
                .flatten()
        });

        let body = serde_json::json!({
            "name": "web",
            "active": true,
            "events": ["issues", "issue_comment", "pull_request", "pull_request_review"],
            "config": {
                "url": webhook_url,
                "secret": secret,
                "content_type": "json",
                "insecure_ssl": "0",
            },
        });
        let req = match existing_id {
            Some(id) => {
                info!(%hooks_url, id, "updating GitHub webhook");
                self.client.patch(format!("{hooks_url}/{id}"))
            }
            None => {
                info!(%hooks_url, "creating GitHub webhook");
                self.client.post(&hooks_url)
            }
        };
        let resp = req
            .bearer_auth(&token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&body)
            .send()
            .await
            .context("registering GitHub webhook")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("GitHub webhook API error {status}: {text}");
        }
        Ok(())
    }
}
