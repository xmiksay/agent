# Project Agent Instructions

This project is managed by an automated GitLab Claude Agent.
You are running as an autonomous agent — there is no human in the loop.

## Workflow

You will receive a task prompt describing what to do (implement issue, review MR, fix review comments, or respond to a comment). Follow the instructions in the prompt carefully.

## Allowed Tools & Commands

You have access to a limited set of shell commands. Do NOT attempt commands outside this list — they will be rejected.

### Git
`git status`, `git add`, `git commit`, `git push`, `git checkout`, `git branch`, `git diff`, `git log`, `git stash`

### GitLab CLI
`glab mr` (create, note, approve, view), `glab issue` (note, view)

### Build & Test
`cargo build`, `cargo check`, `cargo test`, `cargo fmt`, `cargo clippy`, `npm run`, `npm test`, `npx`, `make`

### File reading
`cat`, `head`, `tail`, `wc`, `grep`, `rg`, `find`, `ls`, `pwd`, `which`, `test`

### File writing
`mkdir -p` (create directories only)

Use the built-in `Read`, `Write`, `Edit`, `Glob`, `Grep` tools for file operations instead of shell commands when possible.

### Explicitly DENIED
- `rm` — you cannot delete files or directories
- `git push --force` — no force pushing
- `git reset --hard` — no hard resets

## General Rules

- Always read project files first to understand the codebase before making changes
- Run tests and build checks before committing
- Write clear, concise commit messages
- Do not modify files unrelated to the task
- Do not add unnecessary dependencies
- Keep changes minimal and focused on the task

## Git Workflow

- For new work: create a feature branch from the default branch
- Branch naming: `claude/<issue-number>-<short-description>` or `claude/fix-<mr-number>`
- Commit often with meaningful messages
- Push to remote when done

## Creating Merge Requests

When implementing an issue, create a MR with:

```bash
glab mr create --fill --assignee <issue-author> --title "Resolve #<iid>: <title>"
```

## Posting Reviews

When reviewing a MR, post your review as a comment:

```bash
glab mr note <iid> --message "your review here"
```

If the MR looks good, approve it:

```bash
glab mr approve <iid>
```

## Responding to Comments

When responding to a comment on a MR or issue, use:

```bash
glab mr note <iid> --message "response"
glab issue note <iid> --message "response"
```
