# Project Instructions

This repository is managed (in part) by an automated **Claude Agent** that
listens to webhook events and runs Claude Code sessions to implement issues,
review merge/pull requests, and respond to comments.

When you are reading this file as the agent, you are operating **without a
human at the keyboard**. There is an operator on call who can approve or deny
individual shell commands, but they will not be answering chat-style
clarifying questions in real time. Make reasonable judgment calls and proceed.

## Operating model

- You receive a single task prompt per webhook event. The prompt contains the
  issue/MR/comment text and short trigger-specific instructions for replying.
  The relevant context lives in that prompt and in the repo files; nothing
  else is injected.
- Every `Bash` invocation is gated by an operator-in-the-loop hook. Commands
  that match this project's `allowed_operations` allowlist run immediately.
  Anything else queues for operator approval, which may take minutes — so
  prefer allowlisted operations and avoid clever shell trickery (no command
  substitution `$(...)` / backticks, no `< file`, no `> file`, no process
  substitution; these are rejected statically).
- Use the built-in `Read`, `Write`, `Edit`, `Glob`, `Grep` tools for file work
  whenever possible — they bypass the Bash allowlist entirely.

## Git workflow

- The repo is cloned via SSH (`git@host:path.git`); the agent host has keys
  configured. You do not need to authenticate.
- For new work, branch off the default branch:
  `git checkout -b claude/<issue-iid>-<short-slug>` or
  `git checkout -b claude/fix-<mr-iid>`.
- Commit often with meaningful messages.
- Push with plain `git push -u origin <branch>` — `git push --force` and
  `git reset --hard` are hard-denied; do not attempt them.

## Implementing an issue

When the trigger is an issue assigned to the bot:

1. Read the issue body carefully and explore the relevant source files.
2. Implement the change on a fresh branch.
3. Run the project's verification commands (e.g. `cargo check`, `cargo test`,
   `npm test`) before committing.
4. Commit, push, and open an MR:
   ```
   glab mr create --fill --assignee <issue-author> --title "Resolve #<iid>: <title>"
   ```

## Reviewing a merge request

When the trigger is a review request:

1. `git diff <target>...<source>` to see the change.
2. Post your review as a single comment via
   `glab mr note <iid> --message "..."`. Reference file paths and line numbers
   specifically.
3. If the MR is ready, post a separate approval comment explaining why, then
   `glab mr approve <iid>`. Do not approve silently.
4. Never merge — only humans merge.

## Responding to comments

When the trigger is a comment on an MR or issue:

1. Read the comment in full and look at the surrounding thread for context.
2. Either make the requested change (commit + push) or reply with a
   clarification:
   - `glab mr note <mr-iid> --message "..."`
   - `glab issue note <issue-iid> --message "..."`

## Things to avoid

- Don't `git add -A` or `git add .` unless you have just verified that every
  untracked file is intentional. Prefer explicit paths.
- Don't add dependencies that aren't needed for the task.
- Don't refactor surrounding code beyond what the task requires.
- Don't write speculative tests for behavior the task doesn't introduce.
- Don't close issues or MRs.

## Project-specific conventions

> **Replace this section** with the conventions specific to your project:
> build commands, test commands, lint commands, branch naming, commit-message
> format, code-style rules, etc.

- Build: _e.g._ `cargo check --all-targets`
- Test: _e.g._ `cargo test --all-features`
- Lint: _e.g._ `cargo clippy --all-targets -- -D warnings`
- Format: _e.g._ `cargo fmt --check`
