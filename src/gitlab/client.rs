use anyhow::{Context, Result};
use tracing::info;

#[derive(Clone)]
pub struct GitLabClient {
    client: reqwest::Client,
    base_url: String,
    token: String,
}

impl GitLabClient {
    pub fn new(base_url: &str, token: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    pub async fn post_note(
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

        let resp = self
            .client
            .post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&serde_json::json!({ "body": body }))
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
