# CLAUDE.md — agent project context

This file is the project-specific brief loaded into Claude Code sessions that work *on this repo*. Read it before making changes.

Note: this is **not** the CLAUDE.md the agent ships into the worktrees it manages. The agent never writes `CLAUDE.md` or `.claude/` into project checkouts — those repos own their own context (see memory: *No agent files in worktrees*).

## What this project is

A Rust/Axum HTTP service that listens for GitLab + GitHub webhooks and runs the local `claude` CLI against the affected repository. Output is parsed from `--output-format stream-json` and posted back as an issue/MR/PR comment. A Vue 3 SPA on the same port shows live task status, captured stdout, branch diff, and pending operator approvals.

Single-operator deployment by design — there is no multi-tenancy. Bearer-token auth on `/api/*` is the only access control.

## Engineering rules (apply to every change in this repo)

- **KISS.** Prefer the most direct expression. No premature abstraction, no future-proofing scaffolds, no DI-flavored indirection where a plain function works. Three similar lines beats a clever helper.
- **DRY.** If the same logic is starting to appear in two places, extract it — but only after the second occurrence, not before.
- **File size cap: 500 lines.** When a `.rs`/`.vue`/`.ts` file crosses 500 lines, split it along a natural seam (per-route handlers, per-trigger workflows, per-component slot). `src/jobs/store.rs` (682) and `src/jobs/runner.rs` (544) currently violate this — they should be split as soon as a non-trivial change lands on them. Don't grow them further.
- **Auto-update this file when architecture changes.** If a change adds/removes/renames a module, route, entity, environment variable, or the way two modules talk to each other, edit the relevant section of `CLAUDE.md` in the same PR. The architecture sections below should always describe the current code, not an aspirational shape.
- **Verify before declaring done.** `cargo check` after Rust changes, `npm run typecheck` (in `frontend/`) after TS/Vue changes. UI changes ideally exercised in a browser.
- **Comments: WHY, not WHAT.** Only write a comment when removing it would confuse a future reader (subtle invariant, surprising behavior, deliberate workaround). Don't narrate code.
- **No backwards-compat shims for internal callers.** Rename, delete, and rewrite freely — the API surface that matters is the HTTP routes and the DB schema (and those are governed by migrations).

## Architecture

```
GitLab / GitHub
    │ webhook
    ▼
src/webhook/{gitlab,github}.rs   verify signature, parse provider payload
    │ NormalizedEvent
    ▼
src/webhook/dispatch.rs           dedupe by event_id, decide trigger,
    │                             upsert project, create task row
    ▼
src/jobs/store.rs                 persists tasks, owns the run loop;
    │                             confirm_task → spawn run_job
    ▼
src/jobs/runner.rs                clone/fetch, write settings.local.json,
    │                             spawn `claude -p ... --output-format
    │                             stream-json`, stream into output_log,
    │                             enforce token budget, push changes,
    │                             post result back via GitProvider
    │
    │ PreToolUse hook (Bash/AskUserQuestion)
    ▼
POST /internal/authcheck          src/auth/handlers.rs — loopback-only;
    │                             allowlist match OR open auth_requests
    │                             row and block on AuthWaiter notify
    ▼
Operator (SPA)                    approves/denies via
                                  POST /api/auth_requests/{id}/resolve
```

### Module map

| Path | Responsibility |
|---|---|
| `src/main.rs` | wires `AppState`, runs migrations, registers routes, starts axum |
| `src/config.rs` | env → `Config` struct |
| `src/webhook/gitlab.rs`, `src/webhook/github.rs` | per-provider HTTP handlers: HMAC verify, payload → `NormalizedEvent`, hand to `dispatch` |
| `src/webhook/normalized.rs` | provider-agnostic event shape (`EventKind`) |
| `src/webhook/dispatch.rs` | event → `TriggerReason`, dedupe, project upsert, task create |
| `src/webhook/types.rs` | provider-specific payload structs |
| `src/jobs/types.rs` | `TriggerReason`, `ClaudeOutput` |
| `src/jobs/store.rs` | `TaskStore` — task CRUD, run loop, kill/continue/retry/push_message, branch_diff. **Over 500 lines — split before adding new methods.** |
| `src/jobs/runner.rs` | `run_job` — actually spawns `claude`, streams stdout, enforces token budget, pushes commits, posts the result note. **Over 500 lines.** |
| `src/jobs/registry.rs` | `RunningTasks` — abort handles by task id |
| `src/jobs/output_log.rs` | in-memory stdout/stderr ring (lost on restart by design) |
| `src/workspace/mod.rs` | filesystem layout: `<base>/<service_slug>/<project_slug>/branches/<branch_slug>/` |
| `src/workspace/git.rs` | `clone_or_fetch` |
| `src/workspace/lock.rs` | per-project advisory file lock |
| `src/workspace/layout.rs` | `slugify` |
| `src/provider/mod.rs` | `GitProvider` trait + `NoteTarget` |
| `src/provider/registry.rs` | `ProviderRegistry` — per-service `Arc<dyn GitProvider>` cache, kept in sync with `git_services` table |
| `src/provider/{gitlab,github}/` | provider impls: REST calls for posting notes / approving / reading comments |
| `src/git_service/store.rs` | CRUD for the `git_services` table |
| `src/project/store.rs` | projects + project_branches tables, allowed_operations config |
| `src/api/*.rs` | HTTP handlers under `/api/` — tasks, projects, git_services, auth_requests |
| `src/auth/middleware.rs` | bearer-token check for `/api/*` |
| `src/auth/handlers.rs` | `/internal/authcheck` — loopback-only endpoint that the Claude Code PreToolUse hook calls |
| `src/auth/store.rs` | `auth_requests` CRUD + status enum |
| `src/auth/waiter.rs` | in-process `Notify` map; `authcheck` parks here until the operator resolves |
| `src/auth/operations.rs` | glob matcher for allowlist evaluation |
| `src/entity/*.rs` | SeaORM `Model` structs (one per table) |
| `migration/src/*.rs` | SeaORM migrations — append-only, numbered `mYYYYMMDD_NNNNNN_*` |
| `defaults/.claude/hooks/authcheck.sh` | the PreToolUse hook script, embedded at build via `include_str!` and written into each worktree at `<base>/.agent-hooks/authcheck.sh` |
| `src/spa.rs` | `rust-embed` handler — bakes `frontend/dist/` into the binary and serves it as the `/`-fallback (SPA paths fall through to `index.html`) |
| `frontend/` | Vue 3 + Vite + Pinia + Tailwind SPA — `npm run build` must run before `cargo build` so the bundle is on disk when the embed derive picks it up |

### Database

PostgreSQL via SeaORM. Migrations live in `migration/src/` and run automatically on startup (`migration::Migrator::up(&db, None)` in `main.rs`). Adding a column means a new migration file; never mutate an old one.

Tables (current set, see migration files for canonical schemas):

- `tasks` — one row per agent run; `status` is one of `pending|running|completed|failed|killed`
- `task_results` — final cost / turns / tokens / result text; one-to-one with tasks
- `projects` — discovered repos, per-project `allowed_operations` glob list
- `project_branches` — branches the agent has touched, with `issue_iid` / `pr_iid` linkage and status
- `auth_requests` — operator-approval items raised from the authcheck hook
- `git_services` — provider config: kind, base URL, bot username, PAT, webhook secret

### HTTP surface

Bearer-auth gates `/api/*` (and the SPA, when `API_BEARER_TOKEN` is set). `/webhook/*`, `/health`, `/internal/authcheck` are unauthenticated; the authcheck endpoint is additionally restricted to loopback callers.

| Method | Path | Notes |
|---|---|---|
| `POST` | `/webhook/gitlab/{slug}` | `X-Gitlab-Token` = service `webhook_secret` |
| `POST` | `/webhook/github/{slug}` | `X-Hub-Signature-256` HMAC-SHA256 |
| `POST` | `/internal/authcheck` | loopback only; called by the PreToolUse hook |
| `GET` | `/api/tasks` | optional `?status=` |
| `GET`/`DELETE` | `/api/tasks/{id}` | detail + result; DELETE force-kills if running |
| `POST` | `/api/tasks/{id}/confirm` | pending → running |
| `POST` | `/api/tasks/{id}/retry` | clone the task as a new row |
| `POST` | `/api/tasks/{id}/kill` | SIGKILL; preserves session_id for Resume |
| `POST` | `/api/tasks/{id}/continue` | resume via `claude -r <session_id>` |
| `POST` | `/api/tasks/{id}/message` | queue a follow-up prompt; if running, pause+resume immediately |
| `GET` | `/api/tasks/{id}/diff` | `git diff origin/<default_branch>` of the task's worktree (+ untracked listing) |
| `GET` | `/api/tasks/{id}/output` | in-memory stdout/stderr capture |
| `GET` | `/api/projects` / `GET /api/projects/{id}` / `PUT /api/projects/{id}/config` / `GET /api/projects/{id}/branches` | |
| `GET`/`POST` | `/api/git_services` | tokens and webhook secrets are write-only on GET |
| `GET`/`PUT`/`DELETE` | `/api/git_services/{id}` | |
| `GET` | `/api/auth_requests` | optional `?status=`, `?task_id=` |
| `GET`/`POST` | `/api/auth_requests/{id}` / `/api/auth_requests/{id}/resolve` | unblocks the parked authcheck call |
| `GET` | `/api/auth/check` | 204 probe used by the SPA to validate the token |
| `GET` | `/health` | 200 |

### Workspace layout on disk

```
$REPO_BASE_PATH/
├── .agent-hooks/
│   └── authcheck.sh                     # rewritten at every startup
└── <service_slug>/
    └── <project_slug>/
        └── branches/
            ├── .lock                    # advisory file lock per project
            ├── <branch_slug>/           # one git worktree per branch
            │   ├── .git/
            │   └── .claude/settings.local.json   # bypassPermissions + PreToolUse hook
            └── …
```

`slugify` lower-cases and replaces non-alphanumerics with `__`. Each task confirms the worktree exists (clone or fetch+reset), writes `settings.local.json`, then runs `claude -p ... [--resume <sid>] --output-format stream-json --verbose`.

### How an operator approval works

1. Claude tries to run a `Bash` or `AskUserQuestion`.
2. The PreToolUse hook (`defaults/.claude/hooks/authcheck.sh`) POSTs the command to `http://127.0.0.1:<port>/internal/authcheck` with the task's `CLAUDE_TASK_ID`.
3. The handler matches the command against the project's `allowed_operations` glob list (`auth/operations.rs`). On hit it returns `allowed:true` immediately.
4. On miss (or for `AskUserQuestion`), it creates an `auth_requests` row and parks on `AuthWaiter.register(id).notified()` for up to `OPERATOR_TIMEOUT_SECS` (600s).
5. The operator resolves via `POST /api/auth_requests/{id}/resolve`. The store wakes the waiter; the hook gets `{allowed, reply, reason}` and the Claude Code process continues.

## Configuration

Read by `src/config.rs::from_env`. Defaults in parentheses.

| Var | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | required | Postgres DSN |
| `REPO_BASE_PATH` | `/tmp/claude-jobs` | worktree base |
| `LISTEN_ADDR` | `0.0.0.0:3000` | bind address |
| `MAX_CONCURRENT_JOBS` | `3` | Tokio semaphore size |
| `TASK_TOKEN_BUDGET` | `1_000_000` | soft cap; runner kills `claude` when cumulative `output_tokens ≥ budget/2` and the operator can Resume |
| `API_BEARER_TOKEN` | unset | when set, gates `/api/*`; SPA prompts and stores in `localStorage` |
| `RUST_LOG` | `agent=info` | `tracing-subscriber` filter |

The Vue SPA is baked into the binary by `rust-embed` (`src/spa.rs`) at compile time. There is no runtime path override — to swap the bundle, rebuild.

GitLab/GitHub credentials live in the `git_services` table, **not** in env. Managed via `/api/git_services` and the SPA.

## Build / run

```bash
cd frontend && npm install && npm run build   # produce frontend/dist
cargo run                                     # rust-embed bakes the dist in; migrations run on startup
```

Dev loop (hot-reload SPA against an already-running agent):

```bash
cd frontend && npm run dev                    # vite on 5173, with API proxy to the agent
```

After Rust changes: `cargo check`. After frontend changes: `npm run typecheck`.

## Conventions

- **Errors:** `anyhow::Result` everywhere except where typed errors leave the binary (HTTP response codes). `.context("…")` on every I/O boundary.
- **Logging:** `tracing` — `info!` for state transitions, `warn!`/`error!` for things that survived but shouldn't have. Spans not currently used.
- **Concurrency:** Tokio. Per-project work uses `Workspace::lock_project` (in-process `Mutex` + cross-process advisory file lock); per-task work is just owned values.
- **SeaORM:** entity Model is the read shape; mutations go through `ActiveModel` + `Set(...)`. Cross-row consistency relies on individual statements being short — no explicit transactions today.
- **Idempotence:** dedupe at the event level via `seen_events: HashSet<String>` in `TaskStore`, keyed by `TriggerReason::event_id`. The set is in-memory; restarts re-deliver, but `created_at + trigger_data` makes duplicates easy to spot.
- **Comments:** rule above; current code follows it inconsistently — bring new edits into line, don't write new "what" comments.

## Memory rules (for future Claude sessions)

- Clone repos over SSH only — see memory: *SSH only for git clones*.
- Never write `.claude/` or `CLAUDE.md` into the *project* worktrees the agent manages — see memory: *No agent files in worktrees*. (This `CLAUDE.md` is the agent's own, at the agent repo root — that is allowed.)
