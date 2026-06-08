//! Token resolution seam for provider auth.
//!
//! Every consumer that needs a usable access token — the REST clients
//! (`post_note`) and the runner (the `GH_TOKEN`/`GITLAB_TOKEN` the agent
//! inherits) — goes through [`resolve_token`]. The `Pat` flow is wired (it also
//! carries GitLab's Group/Project Access Token bot identity, #10); GitHub App
//! (#9) token minting will be implemented here.

use anyhow::{Result, bail};

use crate::git_service::ServiceCredentials;

/// Resolve a credential into the bearer/access token used for both REST API
/// calls and `gh`/`glab` inside the worktree.
///
/// The GitHub App flow is intentionally not implemented yet — it will mint a
/// short-lived installation token here (JWT → installation access token) and
/// cache it until expiry. GitLab needs no such flow: its bot identity is a
/// Group/Project Access Token resolved straight through the `Pat` arm.
pub async fn resolve_token(creds: &ServiceCredentials) -> Result<String> {
    match creds {
        ServiceCredentials::Pat(token) => Ok(token.clone()),
        ServiceCredentials::GitHubApp(_) => {
            bail!("GitHub App token minting is not implemented yet — see issue #9")
        }
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
    async fn github_app_is_not_implemented_yet() {
        let creds = ServiceCredentials::GitHubApp(GitHubAppConfig {
            app_id: "1".into(),
            private_key: "pem".into(),
            installation_id: "2".into(),
        });
        let err = resolve_token(&creds).await.unwrap_err().to_string();
        assert!(err.contains("#9"), "got: {err}");
    }
}
