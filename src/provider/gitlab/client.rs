use anyhow::{Context, Result};
use async_trait::async_trait;
use tracing::info;
use uuid::Uuid;

use crate::project::ProviderKind;
use crate::provider::{
    BOT_NOTE_MARKER, GitProvider, NoteTarget, resolve_token, webhook_path_marker,
};
use crate::service::ServiceCredentials;

#[derive(Clone)]
pub struct GitLabClient {
    service_id: Uuid,
    client: reqwest::Client,
    base_url: String,
    creds: ServiceCredentials,
}

impl GitLabClient {
    pub fn new(service_id: Uuid, base_url: &str, creds: ServiceCredentials) -> Self {
        Self {
            service_id,
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            creds,
        }
    }

    async fn post_note_raw(
        &self,
        project_path: &str,
        noteable_type: &str,
        noteable_iid: u64,
        body: &str,
    ) -> Result<()> {
        let encoded_path = urlencoding::encode(project_path);
        let url = format!(
            "{}/api/v4/projects/{encoded_path}/{noteable_type}/{noteable_iid}/notes",
            self.base_url,
        );

        info!(%url, "posting note to GitLab");

        let token = resolve_token(&self.creds).await?;
        let stamped = format!("{body}\n\n{BOT_NOTE_MARKER}");
        let resp = self
            .client
            .post(&url)
            .header("PRIVATE-TOKEN", &token)
            .json(&serde_json::json!({ "body": stamped }))
            .send()
            .await
            .context("failed to post note")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitLab API error {status}: {text}");
        }

        Ok(())
    }
}

#[async_trait]
impl GitProvider for GitLabClient {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gitlab
    }

    fn service_id(&self) -> Uuid {
        self.service_id
    }

    async fn post_note(&self, project_path: &str, target: NoteTarget, body: &str) -> Result<()> {
        let (noteable_type, iid) = match target {
            NoteTarget::Issue(iid) => ("issues", iid),
            NoteTarget::MergeRequest(iid) => ("merge_requests", iid),
        };
        self.post_note_raw(project_path, noteable_type, iid, body)
            .await
    }

    async fn ensure_webhook(&self, repo_path: &str, webhook_url: &str, secret: &str) -> Result<()> {
        let token = resolve_token(&self.creds).await?;
        let encoded = urlencoding::encode(repo_path);
        let hooks_url = format!("{}/api/v4/projects/{encoded}/hooks", self.base_url);
        let marker = webhook_path_marker(webhook_url);

        let list = self
            .client
            .get(&hooks_url)
            .header("PRIVATE-TOKEN", &token)
            .send()
            .await
            .context("listing GitLab hooks")?;
        if !list.status().is_success() {
            let status = list.status();
            let text = list.text().await.unwrap_or_default();
            anyhow::bail!("GitLab hooks list error {status}: {text}");
        }
        let hooks: Vec<serde_json::Value> = list.json().await.context("parsing GitLab hooks")?;
        let existing_id = hooks.iter().find_map(|h| {
            let url = h.get("url")?.as_str()?;
            url.ends_with(marker)
                .then(|| h.get("id")?.as_i64())
                .flatten()
        });

        let body = serde_json::json!({
            "url": webhook_url,
            "token": secret,
            "issues_events": true,
            "merge_requests_events": true,
            "note_events": true,
            "enable_ssl_verification": true,
        });
        let resp = match existing_id {
            Some(id) => {
                info!(%hooks_url, id, "updating GitLab webhook");
                self.client
                    .put(format!("{hooks_url}/{id}"))
                    .header("PRIVATE-TOKEN", &token)
                    .json(&body)
                    .send()
                    .await
            }
            None => {
                info!(%hooks_url, "creating GitLab webhook");
                self.client
                    .post(&hooks_url)
                    .header("PRIVATE-TOKEN", &token)
                    .json(&body)
                    .send()
                    .await
            }
        }
        .context("registering GitLab webhook")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitLab webhook API error {status}: {text}");
        }
        Ok(())
    }
}
