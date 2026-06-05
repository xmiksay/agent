use std::path::Path;

use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tracing::{debug, info};

/// Ensure `path` is a checkout of `branch`:
/// - clone the default branch if the repo is missing,
/// - fetch (pruning deleted remotes), then force-checkout `branch` from
///   `origin/<branch>` if it exists remotely, otherwise create it fresh from
///   `origin/<default_branch>`.
///
/// `checkout -f -B` resets both the branch pointer and the worktree to the
/// start ref in one step, so a reused (possibly dirty/stale) worktree always
/// ends up deterministic.
pub async fn clone_or_fetch(
    path: &Path,
    auth_url: &str,
    branch: &str,
    default_branch: &str,
) -> Result<()> {
    let git_dir = path.join(".git");
    if tokio::fs::metadata(&git_dir).await.is_err() {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        info!(path = %path.display(), default_branch, "cloning fresh checkout");
        let output = Command::new("git")
            .arg("clone")
            .arg("--branch")
            .arg(default_branch)
            .arg(auth_url)
            .arg(path)
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

    info!(path = %path.display(), branch, "fetching");
    run_git(path, &["fetch", "origin", "--prune"]).await?;

    let remote_branch = format!("origin/{branch}");
    let start_ref = if git_ref_exists(path, &remote_branch).await? {
        remote_branch
    } else {
        format!("origin/{default_branch}")
    };

    run_git(path, &["checkout", "-f", "-B", branch, &start_ref]).await?;
    Ok(())
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

async fn run_git(path: &Path, args: &[&str]) -> Result<()> {
    debug!(path = %path.display(), ?args, "git");
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
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
