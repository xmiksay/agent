use crate::jobs::types::TriggerReason;

/// Build the `claude -p` prompt for a trigger. The worktree is already checked
/// out on `branch`, so issue prompts tell claude not to create or switch
/// branches.
pub fn build_prompt(trigger: &TriggerReason, branch: &str) -> String {
    match trigger {
        TriggerReason::Issue { iid, title, description, url, .. } => {
            format!(
                "Implement GitLab issue #{iid}: {title}\n\n\
                 Description:\n{description}\n\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - You are already on branch `{branch}`, checked out from the default \
                   branch. Do NOT create or switch branches.\n\
                 - Implement the issue\n\
                 - Commit your changes\n\
                 - Push with `git push -u origin HEAD`\n\
                 - Open a merge request with `glab mr create --source-branch {branch} --fill`"
            )
        }
        TriggerReason::ReviewMR { iid, title, source_branch, target_branch, url, .. } => {
            format!(
                "Review merge request !{iid}: {title}\n\
                 Branch: {source_branch} -> {target_branch}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Review the diff: `git diff {target_branch}...{source_branch}`\n\
                 - Post your review as a comment using `glab mr note {iid}`\n\
                 - If changes are needed, list them clearly\n\
                 - If everything looks good, approve with `glab mr approve {iid}`"
            )
        }
        TriggerReason::FixReview { iid, title, source_branch, url, review_body, .. } => {
            let review_section = if review_body.trim().is_empty() {
                String::new()
            } else {
                format!("\nReview body:\n{review_body}\n")
            };
            format!(
                "Fix review comments on MR !{iid}: {title}\n\
                 Branch: {source_branch}\n\
                 URL: {url}\n\
                 {review_section}\n\
                 Instructions:\n\
                 - Check review comments: `glab mr view {iid} --comments`\n\
                 - Address each comment\n\
                 - Commit and push fixes"
            )
        }
        TriggerReason::MRComment { mr_iid, comment, url, .. } => {
            format!(
                "A reviewer commented on MR !{mr_iid}\n\
                 Comment: {comment}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Treat the comment as a change request against this branch\n\
                 - Make the requested code changes\n\
                 - Commit the changes with a message that references the comment\n\
                 - Push so the MR picks up the new commit\n\
                 - Reply on the thread with `glab mr note {mr_iid}` summarising what changed"
            )
        }
        TriggerReason::IssueComment { issue_iid, comment, url, .. } => {
            format!(
                "A new comment was posted on issue #{issue_iid} (assigned to the bot)\n\
                 Comment: {comment}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - You are already on branch `{branch}` (the existing feature branch for \
                   this issue). Do NOT create or switch branches.\n\
                 - Re-read the issue and any prior conversation to recover context\n\
                 - Treat the comment as additional guidance or a follow-up request\n\
                 - Rework or extend the implementation as needed\n\
                 - Commit and push any code changes with `git push -u origin HEAD`\n\
                 - If no merge request exists yet for `{branch}`, open one with \
                   `glab mr create --source-branch {branch} --fill`\n\
                 - Reply to the comment using `glab issue note {issue_iid}` summarising what changed"
            )
        }
    }
}
