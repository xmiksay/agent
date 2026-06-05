# Project bootstrap

Files in this directory are **templates** for a repository that you want the
agent to operate on. None of them are loaded by the agent at runtime — they're
copied into a target repo once, during setup, and then live under that repo's
source control.

## What the agent does for you automatically

When a webhook fires, the agent:

1. Clones (or fetches) the target repo into its workspace using its **SSH**
   remote (`git@host:path.git`). The agent host already has SSH keys configured
   for the bot user; no token injection.
2. Writes `.claude/settings.local.json` into the worktree, pointing Claude Code
   at the shared authcheck hook (which lives outside any worktree, at
   `<repo_base_path>/.agent-hooks/authcheck.sh`).
3. Launches Claude Code with a focused, trigger-specific prompt (issue body, MR
   diff context, comment text, etc.). The agent does **not** prepend any
   agent-wide preamble to the prompt — context comes from the issue/MR/comment
   itself plus whatever the project repo has committed (`CLAUDE.md`, README,
   etc., auto-loaded by Claude Code from cwd).
4. Routes every `Bash` and `AskUserQuestion` tool call through `/internal/authcheck`,
   which approves automatically when the command matches the project's
   `allowed_operations` glob list, or queues an operator approval otherwise.

## What you do once, per repo

Copy these files into the target repo, commit them, and configure the project's
allowlist via the agent's admin UI / API.

| File | Where it goes | Commit? |
| --- | --- | --- |
| `CLAUDE.md` | Repo root | yes |
| `.claude/settings.json` | Repo root | yes |
| `.claude/rules/gitlab-workflow.md` | Repo root | yes |
| `.gitignore` (entry) | Append to repo's existing `.gitignore` | yes |

After committing, on the agent host:

1. Add the forge's SSH host key to the agent user's `known_hosts`
   (`ssh-keyscan gitlab.example.com >> ~/.ssh/known_hosts`).
2. Ensure the bot user's SSH key is uploaded to the forge with at least
   Developer/Maintainer access to the target repo.
3. Configure the project's `allowed_operations` in the agent. The DB-default
   list (see `default_allowed_operations()` in `src/project/store.rs`) is a
   reasonable starting point — adjust per-repo as needed.

## File-by-file purpose

- **`CLAUDE.md`** — Project-level instructions that Claude Code reads from the
  repo root on every session. Describes the agent's operating model, the
  workflow expectations, and any project-specific conventions. Read by the
  model on every invocation; keep it under ~200 lines.

- **`.claude/settings.json`** — Committed Claude Code settings shared across
  every developer + the agent. Holds the `permissions.deny` list (hard
  safety: `git push --force`, `git reset --hard`, `rm -rf /*`) and the
  `permissions.allow` list (so interactive `claude` sessions don't prompt for
  every Read/Edit). **Does not** contain the authcheck hook — the agent writes
  that into `.claude/settings.local.json` at task time so it can point at the
  absolute path of the shared hook script.

- **`.claude/rules/gitlab-workflow.md`** — Forge-specific glab/gh conventions.
  Loaded by Claude Code via its rules mechanism.

- **`.gitignore`** — Add `.claude/settings.local.json` (and optionally the
  whole `.claude/cache/` etc.) so the agent-written settings file never
  appears as untracked or gets committed.

## Verifying a project is wired up correctly

After committing the bootstrap files and configuring the agent's project row:

1. Open a test issue assigned to the bot user.
2. Watch the agent logs: you should see "ensuring branch checkout", a clone via
   SSH, and a successful Claude Code launch.
3. In the agent's UI/auth queue, you should see early Bash commands (e.g.
   `git status`, `cargo check`) auto-approve via the allowlist without an
   operator prompt.

If everything Bash hits the operator queue, the project row's
`allowed_operations` is probably empty — that's the most common
"why-is-everything-blocked" cause.
