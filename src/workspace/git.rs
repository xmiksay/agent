use std::path::Path;

use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tracing::{debug, info};

/// Ensure `path` is a checkout of `branch`:
/// - clone the default branch if the repo is missing,
/// - fetch (pruning deleted remotes),
/// - if the worktree is **already on `branch`**, leave it untouched — local
///   commits and uncommitted work are preserved across runs (this is what makes
///   resume-on-message safe),
/// - otherwise check it out from `origin/<branch>` if it exists remotely, else
///   create it fresh from `origin/<default_branch>`.
///
/// We deliberately do **not** force-reset a reused worktree: a per-branch
/// worktree is owned by its task line, so a hard reset would silently discard
/// the agent's in-progress work.
pub async fn clone_or_fetch(
    path: &Path,
    auth_url: &str,
    branch: &str,
    default_branch: &str,
) -> Result<()> {
    let git_dir = path.join(".git");
    let fresh = tokio::fs::metadata(&git_dir).await.is_err();
    if fresh {
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
        format!("origin/{default_branch}")
    };

    run_git(path, &["checkout", "-B", branch, &start_ref]).await?;
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
