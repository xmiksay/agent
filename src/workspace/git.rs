use std::path::Path;

use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tracing::{debug, info};

/// If `path` is already a git repository, `git fetch` + `git checkout` + reset.
/// Otherwise `git clone --branch`.
pub async fn clone_or_fetch(path: &Path, auth_url: &str, branch: &str) -> Result<()> {
    let git_dir = path.join(".git");
    if tokio::fs::metadata(&git_dir).await.is_ok() {
        info!(path = %path.display(), branch, "fetching existing checkout");
        run_git(path, &["fetch", "origin", branch]).await?;
        run_git(path, &["checkout", branch]).await?;
        run_git(path, &["reset", "--hard", &format!("origin/{branch}")]).await?;
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    info!(path = %path.display(), branch, "cloning fresh checkout");
    let output = Command::new("git")
        .arg("clone")
        .arg("--branch")
        .arg(branch)
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
    Ok(())
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
