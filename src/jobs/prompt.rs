use crate::jobs::types::TriggerReason;
use crate::project::ProviderKind;

/// Keeping a long-lived branch current with the default branch. Conservative:
/// only rebase when it applies cleanly, never discard local work. `{default}`
/// is substituted with the project's default branch.
const REBASE_NOTE: &str = "- Before working, bring the branch up to date with the default branch: \
     `git fetch origin && git rebase origin/{default}`. If the rebase \
     conflicts, abort it (`git rebase --abort`) and continue on the current \
     branch — do not force-resolve conflicts blindly or reset the branch.";

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

    /// The provider CLI name (`glab` / `gh`) and the env var its token is in.
    fn cli_auth(&self) -> (&'static str, &'static str) {
        match self.0 {
            ProviderKind::Gitlab => ("glab", "GITLAB_TOKEN"),
            ProviderKind::Github => ("gh", "GH_TOKEN"),
        }
    }
}

/// Build the `claude -p` prompt for a trigger. The worktree is already checked
/// out on `branch`, so issue prompts tell claude not to create or switch
/// branches. `kind` selects provider terminology and CLI (`glab` vs `gh`).
/// When `db_note` is set, a paragraph telling the agent about its throwaway
/// PostgreSQL database is appended (issue #26); the DSN itself stays in env, not
/// here, so the password never lands in persisted `task_events`.
pub fn build_prompt(
    trigger: &TriggerReason,
    branch: &str,
    default_branch: &str,
    kind: ProviderKind,
    db_note: bool,
) -> String {
    let c = Cli(kind);
    let service = match kind {
        ProviderKind::Gitlab => "GitLab",
        ProviderKind::Github => "GitHub",
    };
    let rebase = REBASE_NOTE.replace("{default}", default_branch);
    let body = match trigger {
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
                 - Verify your change builds and the project's tests pass before committing.\n\
                 - Commit your changes\n\
                 - Push with `git push -u origin HEAD`\n\
                 - Open a {pr_noun} with `{create}`, and make sure its description ends \
                   with the line `Closes #{iid}` so the {pr_abbr} is linked to this issue \
                   and the issue auto-closes when the {pr_abbr} is merged.",
                pr_noun = c.pr_noun(),
                pr_abbr = c.pr_abbr(),
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
                 - Review the diff: `git fetch origin && git diff origin/{target_branch}...origin/{source_branch}`\n\
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
                 {rebase}\n\
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
                 {rebase}\n\
                 - Decide whether the comment is a genuine, actionable change request for \
                   this branch. If it is small talk, a question you can answer, or out of \
                   scope, just reply on the thread with `{note}` and make no code changes. \
                   Otherwise, treat it as a change request and:\n\
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
                 {rebase}\n\
                 - Re-read the issue and any prior conversation to recover context\n\
                 - Judge whether the comment asks for real changes. If it is just a \
                   question or acknowledgement, answer it with `{note}` and stop. \
                   Otherwise treat it as additional guidance and rework or extend the \
                   implementation as needed.\n\
                 - Commit and push any code changes with `git push -u origin HEAD`\n\
                 - If no {pr_noun} exists yet for `{branch}`, open one with `{create}` and \
                   ensure its description ends with `Closes #{issue_iid}` (links the \
                   {pr_abbr} to the issue and auto-closes it on merge).\n\
                 - Reply to the comment using `{note}` summarising what changed",
                pr_noun = c.pr_noun(),
                pr_abbr = c.pr_abbr(),
                create = c.create_pr(branch),
                note = c.note_issue(*issue_iid),
            )
        }
    };

    // CLI/transport note (once): the provider CLI is pre-authenticated and the
    // existing `origin` pushes over token-HTTPS, so the agent must not touch auth.
    let (cli, token_var) = c.cli_auth();
    let mut prompt = format!(
        "{body}\n\n\
         Note: the `{cli}` CLI is already authenticated (its token is in \
         `${token_var}`) and `git push -u origin HEAD` over the existing `origin` \
         remote works. Do NOT add SSH keys, configure tokens, or rewrite the remote."
    );

    if db_note {
        prompt.push_str(
            "\n\n\
             Note: a dedicated, throwaway PostgreSQL database is provisioned for this \
             task — its DSN is in `$DATABASE_URL` and the standard `PGHOST`/`PGPORT`/\
             `PGDATABASE`/`PGUSER`/`PGPASSWORD` vars are set, so `psql` with no arguments \
             connects. Use it freely for migrations, fixtures, and tests; it is destroyed \
             automatically when the task ends. Do NOT create or drop databases or roles \
             yourself, and do NOT hard-code these credentials anywhere.",
        );
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue() -> TriggerReason {
        TriggerReason::Issue {
            iid: 7,
            title: "demo".into(),
            description: "body".into(),
            url: "http://x/7".into(),
        }
    }

    fn issue_comment() -> TriggerReason {
        TriggerReason::IssueComment {
            issue_iid: 7,
            comment: "please tweak".into(),
            url: "http://x/7".into(),
        }
    }

    fn mr_comment() -> TriggerReason {
        TriggerReason::MRComment {
            mr_iid: 3,
            comment: "looks off".into(),
            source_branch: "7-demo".into(),
            url: "http://x/3".into(),
        }
    }

    fn fix_review() -> TriggerReason {
        TriggerReason::FixReview {
            iid: 3,
            title: "demo".into(),
            source_branch: "7-demo".into(),
            url: "http://x/3".into(),
            review_body: "fix it".into(),
        }
    }

    fn review_mr() -> TriggerReason {
        TriggerReason::ReviewMR {
            iid: 3,
            title: "demo".into(),
            source_branch: "7-demo".into(),
            target_branch: "main".into(),
            url: "http://x/3".into(),
        }
    }

    #[test]
    fn db_paragraph_present_only_when_requested() {
        let without = build_prompt(&issue(), "7-demo", "main", ProviderKind::Github, false);
        assert!(!without.contains("throwaway PostgreSQL"));
        assert!(!without.contains("$DATABASE_URL"));

        let with = build_prompt(&issue(), "7-demo", "main", ProviderKind::Github, true);
        assert!(with.contains("throwaway PostgreSQL"));
        assert!(with.contains("$DATABASE_URL"));
        // The DSN value itself is never embedded — only the env var name.
        assert!(with.contains("PGPASSWORD"));
        // The base prompt is unchanged otherwise (the DB note is appended).
        assert!(with.starts_with(&without));
    }

    #[test]
    fn issue_links_pr_to_issue_for_auto_close() {
        let gh = build_prompt(&issue(), "7-demo", "main", ProviderKind::Github, false);
        assert!(gh.contains("Closes #7"));
        assert!(gh.contains("gh pr create"));
        // The issue arm tells the agent to verify before committing.
        assert!(gh.contains("tests pass before committing"));

        let gl = build_prompt(&issue(), "7-demo", "main", ProviderKind::Gitlab, false);
        assert!(gl.contains("Closes #7"));
        assert!(gl.contains("glab mr create"));
    }

    #[test]
    fn long_lived_branch_triggers_rebase_onto_default() {
        for t in [issue_comment(), mr_comment(), fix_review()] {
            let p = build_prompt(&t, "7-demo", "main", ProviderKind::Github, false);
            assert!(
                p.contains("git rebase origin/main"),
                "expected rebase note in: {p}"
            );
        }
    }

    #[test]
    fn issue_comment_links_pr_to_issue() {
        let p = build_prompt(
            &issue_comment(),
            "7-demo",
            "main",
            ProviderKind::Github,
            false,
        );
        assert!(p.contains("Closes #7"));
    }

    #[test]
    fn review_mr_is_read_only_and_uses_origin_refs() {
        let p = build_prompt(&review_mr(), "7-demo", "main", ProviderKind::Github, false);
        assert!(!p.contains("git rebase"));
        assert!(p.contains("origin/"));
    }

    #[test]
    fn comment_triggers_judge_relevance_before_editing() {
        let mr = build_prompt(&mr_comment(), "7-demo", "main", ProviderKind::Github, false);
        assert!(mr.contains("Decide whether"));

        let ic = build_prompt(
            &issue_comment(),
            "7-demo",
            "main",
            ProviderKind::Github,
            false,
        );
        assert!(ic.contains("Judge whether"));
    }
}
