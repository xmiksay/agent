//! Token resolution seam for provider auth.
//!
//! Every consumer that needs a usable access token — the REST clients
//! (`post_note`) and the runner (the `GH_TOKEN`/`GITLAB_TOKEN` the agent
//! inherits) — goes through [`resolve_token`]. Today only the `Pat` flow is
//! wired; this is the single place where GitHub App (#9) and GitLab OAuth app
//! (#10) token minting/refresh will be implemented.

use anyhow::{Result, bail};

use crate::git_service::ServiceCredentials;

/// Resolve a credential into the bearer/access token used for both REST API
/// calls and `gh`/`glab` inside the worktree.
///
/// App flows are intentionally not implemented yet — they will mint a
/// short-lived token here (GitHub: JWT → installation access token; GitLab:
/// refresh-token → access token) and cache it until expiry.
pub async fn resolve_token(creds: &ServiceCredentials) -> Result<String> {
    match creds {
        ServiceCredentials::Pat(token) => Ok(token.clone()),
        ServiceCredentials::GitHubApp { .. } => {
            bail!("GitHub App token minting is not implemented yet — see issue #9")
        }
        ServiceCredentials::GitLabOAuth { .. } => {
            bail!("GitLab OAuth token refresh is not implemented yet — see issue #10")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pat_resolves_to_its_token() {
        let token = resolve_token(&ServiceCredentials::Pat("abc".into()))
            .await
            .unwrap();
        assert_eq!(token, "abc");
    }

    #[tokio::test]
    async fn github_app_is_not_implemented_yet() {
        let creds = ServiceCredentials::GitHubApp {
            app_id: "1".into(),
            private_key: "pem".into(),
            installation_id: "2".into(),
        };
        let err = resolve_token(&creds).await.unwrap_err().to_string();
        assert!(err.contains("#9"), "got: {err}");
    }

    #[tokio::test]
    async fn gitlab_oauth_is_not_implemented_yet() {
        let creds = ServiceCredentials::GitLabOAuth {
            client_id: "1".into(),
            client_secret: "s".into(),
            refresh_token: "r".into(),
        };
        let err = resolve_token(&creds).await.unwrap_err().to_string();
        assert!(err.contains("#10"), "got: {err}");
    }
}
