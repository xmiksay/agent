---
description: Rules for working with GitLab via glab CLI
globs: "*"
---

# GitLab Workflow Rules

- Always use `glab` CLI for GitLab interactions, never direct API calls
- Authenticate via environment — `glab` is pre-configured, do not run `glab auth`
- When creating a MR, always use `--fill` to auto-populate from commits
- When posting review comments, be specific — reference file paths and line numbers
- Never close issues or merge MRs — only humans do that
- When approving a MR, first explain why it's ready in a comment
