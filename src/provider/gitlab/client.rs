use anyhow::{Context, Result};
use async_trait::async_trait;
use tracing::info;
use uuid::Uuid;

use crate::project::ProviderKind;
use crate::provider::{BOT_NOTE_MARKER, GitProvider, NoteTarget};

#[derive(Clone)]
pub struct GitLabClient {
    service_id: Uuid,
    client: reqwest::Client,
    base_url: String,
    token: String,
}

impl GitLabClient {
    pub fn new(service_id: Uuid, base_url: &str, token: &str) -> Self {
        Self {
            service_id,
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
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

        let stamped = format!("{body}\n\n{BOT_NOTE_MARKER}");
        let resp = self
            .client
            .post(&url)
            .header("PRIVATE-TOKEN", &self.token)
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
}
