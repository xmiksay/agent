use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tracing::{info, warn, error};

use crate::config::Config;
use crate::gitlab::client::GitLabClient;
use crate::jobs::types::{ClaudeOutput, TriggerReason};

pub async fn run_job(
    trigger: TriggerReason,
    git_url: String,
    project_path: String,
    default_branch: String,
    config: Config,
    gitlab: GitLabClient,
) -> Result<ClaudeOutput> {
    let job_id = uuid::Uuid::new_v4().to_string();
    let work_dir = format!("{}/{}", config.repo_base_path, job_id);

    info!(job_id, "cloning repository");

    let branch = trigger.branch().unwrap_or(&default_branch);
    let clone_status = Command::new("git")
        .args(["clone", "--depth", "1", "--branch", branch, &git_url, &work_dir])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .await
        .context("failed to spawn git clone")?;

    if !clone_status.success() {
        bail!("git clone failed for {git_url} branch {branch}");
    }

    ensure_claude_config(&work_dir).await?;

    let prompt = build_prompt(&trigger);
    info!(job_id, %prompt, "running claude");

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(config.job_timeout_secs),
        Command::new("claude")
            .args([
                "-p", &prompt,
                "--output-format", "json",
                "--max-turns", "50",
            ])
            .current_dir(&work_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await
    .context("claude job timed out")?
    .context("failed to spawn claude")?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    let claude_result: ClaudeOutput = serde_json::from_str(&stdout)
        .context("failed to parse claude output")?;

    info!(
        job_id,
        cost = claude_result.total_cost_usd,
        turns = claude_result.num_turns,
        is_error = claude_result.is_error,
        "claude finished"
    );

    if claude_result.is_error {
        error!(job_id, result = %claude_result.result, "claude returned error");
    }

    // Post result back to GitLab
    post_result(&trigger, &claude_result, &project_path, &gitlab).await?;

    // Push any changes for issue/fix workflows
    if matches!(trigger, TriggerReason::Issue { .. } | TriggerReason::FixReview { .. }) {
        push_changes(&work_dir).await?;
    }

    // Cleanup
    if let Err(e) = tokio::fs::remove_dir_all(&work_dir).await {
        error!(job_id, error = %e, "failed to cleanup work dir");
    }

    Ok(claude_result)
}

fn build_prompt(trigger: &TriggerReason) -> String {
    match trigger {
        TriggerReason::Issue { iid, title, description, url, .. } => {
            format!(
                "Implement GitLab issue #{iid}: {title}\n\n\
                 Description:\n{description}\n\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Read CLAUDE.md for project context\n\
                 - Implement the issue\n\
                 - Create a new branch and commit your changes\n\
                 - Create a merge request using `glab mr create`"
            )
        }
        TriggerReason::ReviewMR { iid, title, source_branch, target_branch, url, .. } => {
            format!(
                "Review merge request !{iid}: {title}\n\
                 Branch: {source_branch} -> {target_branch}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Read CLAUDE.md for project context\n\
                 - Review the diff: `git diff {target_branch}...{source_branch}`\n\
                 - Post your review as a comment using `glab mr note {iid}`\n\
                 - If changes are needed, list them clearly\n\
                 - If everything looks good, approve with `glab mr approve {iid}`"
            )
        }
        TriggerReason::FixReview { iid, title, source_branch, url, .. } => {
            format!(
                "Fix review comments on MR !{iid}: {title}\n\
                 Branch: {source_branch}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Read CLAUDE.md for project context\n\
                 - Check review comments: `glab mr view {iid} --comments`\n\
                 - Address each comment\n\
                 - Commit and push fixes"
            )
        }
        TriggerReason::MRComment { mr_iid, comment, url, .. } => {
            format!(
                "Respond to comment on MR !{mr_iid}\n\
                 Comment: {comment}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Read CLAUDE.md for project context\n\
                 - Address the request in the comment\n\
                 - Reply using `glab mr note {mr_iid}`"
            )
        }
        TriggerReason::IssueComment { issue_iid, comment, url, .. } => {
            format!(
                "Respond to comment on issue #{issue_iid}\n\
                 Comment: {comment}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Read CLAUDE.md for project context\n\
                 - Address the request in the comment\n\
                 - Reply using `glab issue note {issue_iid}`"
            )
        }
    }
}

async fn post_result(
    trigger: &TriggerReason,
    result: &ClaudeOutput,
    project_path: &str,
    gitlab: &GitLabClient,
) -> Result<()> {
    let (noteable_type, noteable_iid) = match trigger {
        TriggerReason::Issue { iid, .. } | TriggerReason::IssueComment { issue_iid: iid, .. } => {
            ("issues", *iid)
        }
        TriggerReason::ReviewMR { iid, .. }
        | TriggerReason::FixReview { iid, .. }
        | TriggerReason::MRComment { mr_iid: iid, .. } => ("merge_requests", *iid),
    };

    let status = if result.is_error { "error" } else { "completed" };
    let body = format!(
        "**Claude Agent** ({status})\n\n\
         Cost: ${:.4} | Turns: {}\n\n\
         {}",
        result.total_cost_usd,
        result.num_turns,
        if result.is_error {
            format!("Error: {}", result.result)
        } else {
            "Task completed. See commits for changes.".to_string()
        }
    );

    gitlab.post_note(project_path, noteable_type, noteable_iid, &body).await
}

async fn push_changes(work_dir: &str) -> Result<()> {
    let has_changes = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(work_dir)
        .output()
        .await?;

    if has_changes.stdout.is_empty() {
        let unpushed = Command::new("git")
            .args(["log", "@{u}..HEAD", "--oneline"])
            .current_dir(work_dir)
            .output()
            .await;

        match unpushed {
            Ok(out) if out.stdout.is_empty() => {
                info!("no changes to push");
                return Ok(());
            }
            _ => {}
        }
    }

    info!("pushing changes");
    let push = Command::new("git")
        .args(["push"])
        .current_dir(work_dir)
        .status()
        .await
        .context("failed to git push")?;

    if !push.success() {
        bail!("git push failed");
    }

    Ok(())
}

const DEFAULTS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/defaults");

async fn ensure_claude_config(work_dir: &str) -> Result<()> {
    let claude_md = Path::new(work_dir).join("CLAUDE.md");
    let claude_dir = Path::new(work_dir).join(".claude");

    if !claude_md.exists() {
        info!("CLAUDE.md not found, copying defaults");
        tokio::fs::copy(
            Path::new(DEFAULTS_DIR).join("CLAUDE.md"),
            &claude_md,
        )
        .await
        .context("failed to copy default CLAUDE.md")?;
    } else {
        info!("CLAUDE.md already exists, keeping project version");
    }

    if !claude_dir.exists() {
        info!(".claude/ not found, copying defaults");
        copy_dir_recursive(
            &Path::new(DEFAULTS_DIR).join(".claude"),
            &claude_dir,
        )
        .await
        .context("failed to copy default .claude/")?;
    } else {
        // Ensure settings.json exists even if .claude/ dir is present
        let settings = claude_dir.join("settings.json");
        if !settings.exists() {
            warn!(".claude/ exists but settings.json missing, copying default");
            tokio::fs::copy(
                Path::new(DEFAULTS_DIR).join(".claude/settings.json"),
                &settings,
            )
            .await
            .context("failed to copy default settings.json")?;
        }
    }

    Ok(())
}

fn copy_dir_recursive<'a>(
    src: &'a Path,
    dst: &'a Path,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        tokio::fs::create_dir_all(dst).await?;

        let mut entries = tokio::fs::read_dir(src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if entry.file_type().await?.is_dir() {
                copy_dir_recursive(&src_path, &dst_path).await?;
            } else {
                tokio::fs::copy(&src_path, &dst_path).await?;
            }
        }

        Ok(())
    })
}
