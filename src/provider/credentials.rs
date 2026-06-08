//! Token resolution seam for provider auth.
//!
//! Every consumer that needs a usable access token — the REST clients
//! (`post_note`) and the runner (the `GH_TOKEN`/`GITLAB_TOKEN` the agent
//! inherits) — goes through [`resolve_token`]. `Pat` returns the stored token
//! directly (it also carries GitLab's Group/Project Access Token bot identity,
//! #10); `GitHubApp` mints + caches a short-lived installation token (#9).

use anyhow::Result;

use crate::git_service::ServiceCredentials;
use crate::provider::github;

/// Resolve a credential into the bearer/access token used for both REST API
/// calls and `gh`/`glab` inside the worktree.
///
/// The `GitHubApp` arm signs an app JWT and exchanges it for an installation
/// access token, cached in-process until ~5 min before expiry (see
/// [`github::app`]). GitLab needs no such flow: its bot identity is a
/// Group/Project Access Token resolved straight through the `Pat` arm.
pub async fn resolve_token(creds: &ServiceCredentials) -> Result<String> {
    match creds {
        ServiceCredentials::Pat(token) => Ok(token.clone()),
        ServiceCredentials::GitHubApp(cfg) => github::app::installation_token(cfg).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_service::GitHubAppConfig;

    #[tokio::test]
    async fn pat_resolves_to_its_token() {
        let token = resolve_token(&ServiceCredentials::Pat("abc".into()))
            .await
            .unwrap();
        assert_eq!(token, "abc");
    }

    #[tokio::test]
    async fn github_app_without_installation_id_is_a_clear_error() {
        // Pre-install service: no installation_id yet → minting can't proceed,
        // and the error tells the operator to run the install flow.
        let creds = ServiceCredentials::GitHubApp(GitHubAppConfig {
            app_id: "1".into(),
            private_key: "pem".into(),
            installation_id: String::new(),
            api_base: "https://api.github.com".into(),
        });
        let err = resolve_token(&creds).await.unwrap_err().to_string();
        assert!(err.contains("not installed"), "got: {err}");
    }
}
