# CLAUDE.md — agent project context

This file is the project-specific brief loaded into Claude Code sessions that work *on this repo*. Read it before making changes.

Note: this is **not** the CLAUDE.md the agent ships into the worktrees it manages. The agent never writes `CLAUDE.md` or `.claude/` into project checkouts — those repos own their own context (see memory: *No agent files in worktrees*).

## What this project is

A Rust/Axum HTTP service that listens for GitLab + GitHub webhooks and runs the local `claude` CLI against the affected repository. Output is parsed from `--output-format stream-json` and posted back as an issue/MR/PR comment. A Vue 3 SPA on the same port shows live task status, captured stdout, branch diff, and pending operator approvals.

Single-operator deployment by design — there is no multi-tenancy. Bearer-token auth on `/api/*` is the only access control.

## Engineering rules (apply to every change in this repo)

- **KISS.** Prefer the most direct expression. No premature abstraction, no future-proofing scaffolds, no DI-flavored indirection where a plain function works. Three similar lines beats a clever helper.
- **DRY.** If the same logic is starting to appear in two places, extract it — but only after the second occurrence, not before.
- **File size cap: 400 lines.** When a `.rs`/`.vue`/`.ts` file crosses 400 lines of *production* code, split it along a natural seam (per-route handlers, per-trigger workflows, per-component slot). `src/jobs/store.rs` was split into `store`/`lifecycle`/`control`/`create`/`queries`; `lifecycle.rs` and `hub.rs` sit above 400 only because of their in-file `#[cfg(test)]` modules — keep the production half under the cap and never grow a file that's already over.
- **Git workflow.** Rebase onto the latest `master` before starting work and never commit to `master` (derive a branch from the issue if none is given). Integrate fast-forward only — no merge commits. Commit/task messages use the **What / Why / How** format. Never add a `Co-Authored-By` trailer. (Full rules: workspace `CLAUDE.md` → *Git Workflow*.)
- **Agents & skills.** Delegate stack-specific work to the user-global `backend` (Rust, unit + integration tests) and `frontend` (Vue/TS) subagents — they read this file for project context. For reviews use the `/code-review` / `/security-review` skills. Verify a change with the project's **`/check`** skill (runs `make verify`).
- **Keep the architecture doc current.** If a change adds/removes/renames a module, route, entity, environment variable, or the way two modules talk to each other, edit the relevant section of [`docs/architecture.md`](../docs/architecture.md) in the same change. It should always describe the current code, not an aspirational shape.
- **Verify before declaring done.** Run the `/check` skill (`make verify`) — lint + tests must pass. For a fast inner loop: `make check` (Rust typecheck), `npm run typecheck` in `frontend/`. UI changes ideally exercised in a browser.
- **Comments: WHY, not WHAT.** Only write a comment when removing it would confuse a future reader (subtle invariant, surprising behavior, deliberate workaround). Don't narrate code.
- **No backwards-compat shims for internal callers.** Rename, delete, and rewrite freely — the API surface that matters is the HTTP routes and the DB schema (and those are governed by migrations).

## Architecture (overview)

Webhook → normalize → dispatch (dedupe, pick trigger, upsert project, create task) → `TaskStore` run loop → `run_job` spawns the `claude` CLI as a **long-lived, turn-based** interactive stream-json session in a per-branch git worktree. Stdout events fan out through a per-task hub to one app-wide `/ws` socket (the SPA) and persist to `task_events`; `can_use_tool` control requests are gated in-process by `handle_permission` (non-Bash auto-allowed, Bash matched against the project allowlist, else an `auth_requests` row the operator resolves). At session end the runner pushes commits and posts a result note via the `GitProvider`.

Two extension seams: `AgentBackend` (the coding-agent CLI — only `ClaudeCode` today) and `GitProvider` (GitLab + GitHub today, Codeberg/Forgejo planned).

> **Full reference — read before touching these areas:** the flow diagram, complete **module map**, **database** schema, **HTTP surface**, **workspace/turn-loop** semantics, and the **operator-approval** protocol all live in [`docs/architecture.md`](../docs/architecture.md).

## Configuration

Read by `src/config.rs::from_env`. Defaults in parentheses.

| Var | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | required | Postgres DSN |
| `REPO_BASE_PATH` | `/tmp/claude-jobs` | worktree base |
| `LISTEN_ADDR` | `0.0.0.0:3000` | bind address |
| `PUBLIC_BASE_URL` | unset | externally reachable base URL; builds the `/webhook/{kind}/{slug}` callback for hook auto-registration. Unset → auto-registration skipped |
| `MAX_CONCURRENT_JOBS` | `3` | Tokio semaphore size — gates **actively-processing turns**, acquired/released per turn so idle warm agents hold no slot |
| `TASK_TOKEN_BUDGET` | `1_000_000` | soft cap; runner kills `claude` when cumulative `output_tokens ≥ budget/2` and the operator can Resume |
| `OPERATOR_APPROVAL_TIMEOUT_SECS` | `0` | seconds before an unanswered tool-approval auto-denies. **`0` = wait indefinitely** (never auto-deny — the default). `>0` auto-denies on timeout, resolving the row + clearing the UI. Tradeoff: a turn parked on approval holds its `MAX_CONCURRENT_JOBS` permit, so indefinite waits can starve other turns |
| `API_BEARER_TOKEN` | unset | when set, gates `/api/*`; SPA prompts and stores in `localStorage` |
| `RUST_LOG` | `agent=info` | `tracing-subscriber` filter |

The Vue SPA is baked into the binary by `rust-embed` (`src/spa.rs`) at compile time. There is no runtime path override — to swap the bundle, rebuild.

GitLab/GitHub credentials live in the `git_services` table, **not** in env. Managed via `/api/git_services` and the SPA.

## Build / run

All build/test/dev flows go through the **`Makefile`** (the SPA must be built before the binary — rust-embed bakes `frontend/dist/` in — and the targets enforce that ordering):

```bash
make run        # build SPA + cargo run (migrations run on startup)
make verify     # the pre-"done" gate: lint (fmt + clippy + typecheck) + tests
make build      # release build (SPA + binary)
make dev        # hot-reload SPA: vite on 5173, API-proxied to the agent
make check      # fast Rust typecheck
```

Node version is pinned in `.nvmrc` (`nvm use` first). `/check` is the skill wrapper around `make verify`.

## Conventions

- **Errors:** `anyhow::Result` everywhere except where typed errors leave the binary (HTTP response codes). `.context("…")` on every I/O boundary.
- **Logging:** `tracing` — `info!` for state transitions, `warn!`/`error!` for things that survived but shouldn't have. Spans not currently used.
- **Concurrency:** Tokio. Per-branch worktree setup uses `Workspace::lock_branch` (in-process `Mutex` + cross-process advisory file lock); tasks on different branches of the same project run concurrently. `confirm_task` blocks only when another task on the **same project+branch** is already running. Per-task work is just owned values.
- **SeaORM:** entity Model is the read shape; mutations go through `ActiveModel` + `Set(...)`. Cross-row consistency relies on individual statements being short — no explicit transactions today.
- **Idempotence:** dedupe at the event level via `seen_events: HashSet<String>` in `TaskStore`, keyed by `TriggerReason::event_id`. The set is in-memory; restarts re-deliver, but `created_at + trigger_data` makes duplicates easy to spot.
- **Bot-comment marker:** every comment posted via `GitProvider::post_note` gets `BOT_NOTE_MARKER` (`<!-- agent -->`) appended. The provider-side webhook normalizers drop incoming notes that contain the marker, so the bot never reacts to its own posts. This is the loop guard — the dispatcher no longer compares actor to `bot_username`, which means a same-account operator/bot setup still works.
- **Comments:** rule above; current code follows it inconsistently — bring new edits into line, don't write new "what" comments.

## Memory rules (for future Claude sessions)

- Runtime git transport for the managed project worktrees is **token-HTTPS** (issue #22): the remote is a secret-free `https://host/path.git` and a git credential helper reads the per-service token from `GH_TOKEN`/`GITLAB_TOKEN` at call time (no token in `.git/config`, no host SSH key). See `src/workspace/git.rs::HttpsAuth`. (Supersedes the earlier *SSH only for git clones* quickfix.)
- Never write `.claude/` or `CLAUDE.md` into the *project* worktrees the agent manages — see memory: *No agent files in worktrees*. (This `CLAUDE.md` is the agent's own, at the agent repo root — that is allowed.)
