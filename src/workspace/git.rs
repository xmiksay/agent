use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use tokio::process::Command;
use tracing::{debug, info};

use crate::project::ProviderKind;

/// HTTPS token transport for a repo. Replaces the old host-SSH dependency: the
/// persisted `origin` is a clean `https://host/path.git` with no secret, and a
/// git credential helper supplies the token from `token_env` at call time — so
/// the token is never written into `.git/config`. Both our own git children
/// (clone/fetch/push) and the agent's own `git push` inherit `token_env`, so
/// both authenticate transparently.
#[derive(Clone)]
pub struct HttpsAuth {
    /// Secret-free remote, e.g. `https://github.com/owner/repo.git`.
    pub remote_url: String,
    /// `git credential.helper` value: a shell snippet that echoes the username
    /// and reads the password from `token_env` at call time.
    pub credential_helper: String,
    /// Env var the helper reads (`GH_TOKEN` / `GITLAB_TOKEN`).
    pub token_env: String,
    /// Token value, exported into our own git children's environment.
    pub token: String,
}

impl HttpsAuth {
    /// Build from the repo's remote URL — accepts either an SSH form
    /// (`git@host:path.git`, `ssh://git@host[:port]/path.git`) or an HTTP(S) form
    /// (`https://host/path.git`, with any `user[:pass]@` userinfo stripped) — plus
    /// the provider kind (selects the HTTPS username + token env var) and a
    /// resolved access token. Both forms normalize to a secret-free
    /// `https://{host}/{path}` remote that the credential helper authenticates.
    pub fn from_remote_url(remote_url: &str, kind: ProviderKind, token: &str) -> Result<Self> {
        let (host, path) = parse_remote_url(remote_url)
            .with_context(|| format!("parsing remote url {remote_url}"))?;
        let remote_url = format!("https://{host}/{path}");
        let (user, token_env) = match kind {
            ProviderKind::Github => ("x-access-token", "GH_TOKEN"),
            ProviderKind::Gitlab => ("oauth2", "GITLAB_TOKEN"),
        };
        // `!f` runs the snippet as a shell command; git appends the operation
        // (get/store/erase) as $1, which we ignore. The token is interpolated by
        // the shell at call time from the env var, so it stays out of .git/config.
        let credential_helper =
            format!("!f() {{ echo username={user}; echo \"password=${token_env}\"; }}; f");
        Ok(Self {
            remote_url,
            credential_helper,
            token_env: token_env.to_string(),
            token: token.to_string(),
        })
    }
}

/// Split a remote into `(host, path-without-leading-slash)`. Handles HTTP(S)
/// (`http(s)://[user[:pass]@]host/path`), the scp-like `[user@]host:path`, and
/// the `ssh://[user@]host[:port]/path` forms.
fn parse_remote_url(url: &str) -> Result<(String, String)> {
    if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        // Strip any `user[:pass]@` userinfo so no secret survives into origin.
        let rest = rest.rsplit_once('@').map(|(_, h)| h).unwrap_or(rest);
        let (authority, path) = rest
            .split_once('/')
            .ok_or_else(|| anyhow!("http url missing path: {url}"))?;
        let host = authority.split(':').next().unwrap_or(authority);
        return Ok((host.to_string(), path.trim_start_matches('/').to_string()));
    }
    parse_ssh_url(url)
}

/// Split an SSH remote into `(host, path-without-leading-slash)`. Handles both
/// the scp-like `[user@]host:path` and the `ssh://[user@]host[:port]/path` forms.
fn parse_ssh_url(url: &str) -> Result<(String, String)> {
    if let Some(rest) = url.strip_prefix("ssh://") {
        let rest = rest.split_once('@').map(|(_, h)| h).unwrap_or(rest);
        let (authority, path) = rest
            .split_once('/')
            .ok_or_else(|| anyhow!("ssh url missing path: {url}"))?;
        let host = authority.split(':').next().unwrap_or(authority);
        return Ok((host.to_string(), path.trim_start_matches('/').to_string()));
    }
    let rest = url.split_once('@').map(|(_, h)| h).unwrap_or(url);
    let (host, path) = rest
        .split_once(':')
        .ok_or_else(|| anyhow!("not an ssh url: {url}"))?;
    // Reject scheme URLs (`https://…`): in scp form the path after `:` is a repo
    // path, never an authority — so a leading `/` (from `://`) means not-scp.
    if host.is_empty() || path.starts_with('/') {
        bail!("not an ssh url: {url}");
    }
    Ok((host.to_string(), path.to_string()))
}

/// Ensure `path` is a checkout of `branch`:
/// - clone the default branch if the repo is missing,
/// - fetch (pruning deleted remotes),
/// - if the worktree is **already on `branch`**, leave it untouched — local
///   commits and uncommitted work are preserved across runs (this is what makes
///   resume-on-message safe),
/// - otherwise check it out from `origin/<branch>` if it exists remotely, else
///   create it fresh from `origin/<base_branch>`.
///
/// We deliberately do **not** force-reset a reused worktree: a per-branch
/// worktree is owned by its task line, so a hard reset would silently discard
/// the agent's in-progress work.
///
/// Transport is token-HTTPS (see [`HttpsAuth`]): every git child gets the token
/// in its environment and the repo's persisted credential helper reads it, so no
/// host SSH key is required and no secret lands in `.git/config`.
pub async fn clone_or_fetch(
    path: &Path,
    auth: &HttpsAuth,
    branch: &str,
    base_branch: &str,
) -> Result<()> {
    let git_dir = path.join(".git");
    let fresh = tokio::fs::metadata(&git_dir).await.is_err();
    if fresh {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        info!(path = %path.display(), base_branch, "cloning fresh checkout");
        let output = Command::new("git")
            .arg("-c")
            .arg(format!("credential.helper={}", auth.credential_helper))
            .arg("clone")
            .arg("--branch")
            .arg(base_branch)
            .arg(&auth.remote_url)
            .arg(path)
            .env(&auth.token_env, &auth.token)
            .output()
            .await
            .context("spawning git clone")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "git clone failed with status {}\nstderr:\n{stderr}",
                output.status
            );
        }
    }

    // Persist the clean remote + credential helper. Idempotent, and it also
    // migrates pre-existing worktrees off the old SSH remote.
    run_git(
        path,
        auth,
        &["remote", "set-url", "origin", &auth.remote_url],
    )
    .await?;
    run_git(
        path,
        auth,
        &["config", "credential.helper", &auth.credential_helper],
    )
    .await?;

    info!(path = %path.display(), branch, "fetching");
    run_git(path, auth, &["fetch", "origin", "--prune"]).await?;

    // Already on the target branch → keep the worktree exactly as it is.
    if !fresh && current_branch(path).await?.as_deref() == Some(branch) {
        debug!(path = %path.display(), branch, "worktree already on branch; keeping as-is");
        return Ok(());
    }

    // Switch to / create the branch. No `-f`: we only get here on a clean fresh
    // clone or a worktree that isn't on the branch, so there is no in-progress
    // work to clobber (and if there somehow is, failing beats destroying it).
    let remote_branch = format!("origin/{branch}");
    let start_ref = if git_ref_exists(path, &remote_branch).await? {
        remote_branch
    } else {
        format!("origin/{base_branch}")
    };

    run_git(path, auth, &["checkout", "-B", branch, &start_ref]).await?;
    Ok(())
}

/// The currently checked-out branch name, or `None` if detached / unknown.
async fn current_branch(path: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .await
        .context("spawning git rev-parse --abbrev-ref HEAD")?;
    if !output.status.success() {
        return Ok(None);
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() || name == "HEAD" {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}

/// True if `refname` resolves (e.g. `origin/feature`). A non-zero exit means
/// "absent", which is a normal outcome here, so this can't go through `run_git`.
async fn git_ref_exists(path: &Path, refname: &str) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--verify", "--quiet", refname])
        .output()
        .await
        .with_context(|| format!("spawning git rev-parse {refname}"))?;
    Ok(output.status.success())
}

/// Run `git -C <path> <args>` with the token in the environment so the persisted
/// credential helper can authenticate networked operations (fetch).
async fn run_git(path: &Path, auth: &HttpsAuth, args: &[&str]) -> Result<()> {
    debug!(path = %path.display(), ?args, "git");
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .env(&auth.token_env, &auth.token)
        .output()
        .await
        .with_context(|| format!("spawning git {args:?}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "git {args:?} failed with status {}\nstderr:\n{stderr}",
            output.status
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_auth_from_scp_style_github() {
        let a = HttpsAuth::from_remote_url(
            "git@github.com:owner/repo.git",
            ProviderKind::Github,
            "tok",
        )
        .unwrap();
        assert_eq!(a.remote_url, "https://github.com/owner/repo.git");
        assert_eq!(a.token_env, "GH_TOKEN");
        assert!(a.credential_helper.contains("username=x-access-token"));
        assert!(a.credential_helper.contains("password=$GH_TOKEN"));
    }

    #[test]
    fn https_auth_from_scp_style_gitlab_subgroup() {
        let a = HttpsAuth::from_remote_url(
            "git@gitlab.example.com:group/sub/repo.git",
            ProviderKind::Gitlab,
            "tok",
        )
        .unwrap();
        assert_eq!(
            a.remote_url,
            "https://gitlab.example.com/group/sub/repo.git"
        );
        assert_eq!(a.token_env, "GITLAB_TOKEN");
        assert!(a.credential_helper.contains("username=oauth2"));
    }

    #[test]
    fn https_auth_from_ssh_scheme_with_port() {
        let a = HttpsAuth::from_remote_url(
            "ssh://git@gitlab.example.com:2222/group/repo.git",
            ProviderKind::Gitlab,
            "tok",
        )
        .unwrap();
        assert_eq!(a.remote_url, "https://gitlab.example.com/group/repo.git");
    }

    #[test]
    fn https_auth_accepts_https_url() {
        let a = HttpsAuth::from_remote_url("https://github.com/o/r.git", ProviderKind::Github, "t")
            .unwrap();
        assert_eq!(a.remote_url, "https://github.com/o/r.git");
        assert_eq!(a.token_env, "GH_TOKEN");
    }

    #[test]
    fn https_auth_strips_userinfo_from_https_url() {
        // An operator-pasted remote may carry `user@` (or `user:pass@`); the
        // persisted origin must come out secret-free.
        let a = HttpsAuth::from_remote_url(
            "https://x-access-token:ghp_secret@github.com/o/r.git",
            ProviderKind::Github,
            "t",
        )
        .unwrap();
        assert_eq!(a.remote_url, "https://github.com/o/r.git");
    }
}
