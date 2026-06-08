use crate::jobs::types::TriggerReason;
use crate::project::ProviderKind;

/// Provider-specific terminology and CLI snippets. GitLab speaks "merge request"
/// (`!iid`) through `glab`; GitHub speaks "pull request" (`#iid`) through `gh`.
struct Cli(ProviderKind);

impl Cli {
    /// "merge request" / "pull request"
    fn pr_noun(&self) -> &'static str {
        match self.0 {
            ProviderKind::Gitlab => "merge request",
            ProviderKind::Github => "pull request",
        }
    }

    /// short form: "MR" / "PR"
    fn pr_abbr(&self) -> &'static str {
        match self.0 {
            ProviderKind::Gitlab => "MR",
            ProviderKind::Github => "PR",
        }
    }

    /// reference sigil for a change request: "!" (GitLab) / "#" (GitHub)
    fn pr_ref(&self) -> &'static str {
        match self.0 {
            ProviderKind::Gitlab => "!",
            ProviderKind::Github => "#",
        }
    }

    /// Command to open a change request from the current branch.
    fn create_pr(&self, branch: &str) -> String {
        match self.0 {
            ProviderKind::Gitlab => format!("glab mr create --source-branch {branch} --fill"),
            ProviderKind::Github => format!("gh pr create --head {branch} --fill"),
        }
    }

    /// Command to post a comment/note on a change request.
    fn note_pr(&self, iid: u64) -> String {
        match self.0 {
            ProviderKind::Gitlab => format!("glab mr note {iid}"),
            ProviderKind::Github => format!("gh pr comment {iid} --body \"...\""),
        }
    }

    /// Command to approve a change request.
    fn approve_pr(&self, iid: u64) -> String {
        match self.0 {
            ProviderKind::Gitlab => format!("glab mr approve {iid}"),
            ProviderKind::Github => format!("gh pr review {iid} --approve"),
        }
    }

    /// Command to read a change request's review comments.
    fn view_pr_comments(&self, iid: u64) -> String {
        match self.0 {
            ProviderKind::Gitlab => format!("glab mr view {iid} --comments"),
            ProviderKind::Github => format!("gh pr view {iid} --comments"),
        }
    }

    /// Command to reply on an issue.
    fn note_issue(&self, iid: u64) -> String {
        match self.0 {
            ProviderKind::Gitlab => format!("glab issue note {iid}"),
            ProviderKind::Github => format!("gh issue comment {iid} --body \"...\""),
        }
    }
}

/// Build the `claude -p` prompt for a trigger. The worktree is already checked
/// out on `branch`, so issue prompts tell claude not to create or switch
/// branches. `kind` selects provider terminology and CLI (`glab` vs `gh`).
pub fn build_prompt(trigger: &TriggerReason, branch: &str, kind: ProviderKind) -> String {
    let c = Cli(kind);
    let service = match kind {
        ProviderKind::Gitlab => "GitLab",
        ProviderKind::Github => "GitHub",
    };
    match trigger {
        TriggerReason::Issue {
            iid,
            title,
            description,
            url,
            ..
        } => {
            format!(
                "Implement {service} issue #{iid}: {title}\n\n\
                 Description:\n{description}\n\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - You are already on branch `{branch}`, checked out from the default \
                   branch. Do NOT create or switch branches.\n\
                 - Implement the issue\n\
                 - Commit your changes\n\
                 - Push with `git push -u origin HEAD`\n\
                 - Open a {pr_noun} with `{create}`",
                pr_noun = c.pr_noun(),
                create = c.create_pr(branch),
            )
        }
        TriggerReason::ReviewMR {
            iid,
            title,
            source_branch,
            target_branch,
            url,
            ..
        } => {
            format!(
                "Review {pr_noun} {pr_ref}{iid}: {title}\n\
                 Branch: {source_branch} -> {target_branch}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Review the diff: `git diff {target_branch}...{source_branch}`\n\
                 - Post your review as a comment using `{note}`\n\
                 - If changes are needed, list them clearly\n\
                 - If everything looks good, approve with `{approve}`",
                pr_noun = c.pr_noun(),
                pr_ref = c.pr_ref(),
                note = c.note_pr(*iid),
                approve = c.approve_pr(*iid),
            )
        }
        TriggerReason::FixReview {
            iid,
            title,
            source_branch,
            url,
            review_body,
            ..
        } => {
            let review_section = if review_body.trim().is_empty() {
                String::new()
            } else {
                format!("\nReview body:\n{review_body}\n")
            };
            format!(
                "Fix review comments on {pr_abbr} {pr_ref}{iid}: {title}\n\
                 Branch: {source_branch}\n\
                 URL: {url}\n\
                 {review_section}\n\
                 Instructions:\n\
                 - Check review comments: `{view}`\n\
                 - Address each comment\n\
                 - Commit and push fixes",
                pr_abbr = c.pr_abbr(),
                pr_ref = c.pr_ref(),
                view = c.view_pr_comments(*iid),
            )
        }
        TriggerReason::MRComment {
            mr_iid,
            comment,
            url,
            ..
        } => {
            format!(
                "A reviewer commented on {pr_abbr} {pr_ref}{mr_iid}\n\
                 Comment: {comment}\n\
                 URL: {url}\n\n\
                 Instructions:\n\
                 - Treat the comment as a change request against this branch\n\
                 - Make the requested code changes\n\
                 - Commit the changes with a message that references the comment\n\
                 - Push so the {pr_abbr} picks up the new commit\n\
                 - Reply on the thread with `{note}` summarising what changed",
                pr_abbr = c.pr_abbr(),
                pr_ref = c.pr_ref(),
                note = c.note_pr(*mr_iid),
            )
        }
        TriggerReason::IssueComment {
            issue_iid,
            comment,
            url,
            ..
        } => {
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
                 - If no {pr_noun} exists yet for `{branch}`, open one with \
                   `{create}`\n\
                 - Reply to the comment using `{note}` summarising what changed",
                pr_noun = c.pr_noun(),
                create = c.create_pr(branch),
                note = c.note_issue(*issue_iid),
            )
        }
    }
}
