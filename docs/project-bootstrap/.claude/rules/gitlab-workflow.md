---
description: Forge workflow conventions for the Claude Agent (GitLab via glab, GitHub via gh)
globs: "*"
---

# Forge workflow rules

These rules apply when the Claude Agent is acting on behalf of a webhook event.

## CLI usage

- Use the `glab` CLI for all GitLab interactions (issues, MRs, comments,
  approvals). Use `gh` for GitHub equivalents (issues, PRs, comments,
  approvals). Never call the REST/GraphQL API directly with `curl`.
- Authentication is pre-configured on the agent host. Do **not** run
  `glab auth login` or `gh auth login`.
- For long output (issue comments, PR diffs, CI logs), pipe through `head -N`
  or use the CLI's own pagination flags (`--limit`, `--per-page`). Avoid
  dumping unbounded text.

## Creating merge / pull requests

- Always use `--fill` so the title and body come from the commits you just
  made. If you need to override, pass `--title` / `--body` explicitly.
- For GitLab:
  `glab mr create --fill --assignee <author> --title "Resolve #<iid>: <title>"`
- For GitHub:
  `gh pr create --fill --assignee <author> --title "Resolve #<iid>: <title>"`
- Do not open draft MRs/PRs unless the trigger explicitly says so.

## Posting reviews

- Post the review as **one** comment, not a chain of small notes.
- Reference file paths and line numbers explicitly, e.g.
  `src/foo.rs:42 — this branch is unreachable because ...`.
- If the change is ready, post a separate approval comment that explains *why*
  it is ready, then call:
  - GitLab: `glab mr approve <iid>`
  - GitHub: `gh pr review --approve <iid>`
  Never approve silently — the explanation comment must come first.
- Never use the CLI's "merge" subcommand. Merging is a human decision.

## Responding to comments

- Quote the relevant bit of the comment you're addressing if the thread is
  long; otherwise just answer directly.
- One reply per agent invocation. If multiple distinct comments need
  responses, post one note per comment.

## Things the agent must never do

- Close issues or MRs/PRs (`glab issue close`, `gh issue close`, etc.).
- Edit issue/MR titles or descriptions after they were opened (the agent
  responds, it doesn't curate).
- Force-push, hard-reset, or rewrite history on branches it didn't create.
- Push directly to the project's default branch.
