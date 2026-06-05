# CLAUDE.md — agent project context

This file is the project-specific brief loaded into Claude Code sessions that work *on this repo*. Read it before making changes.

Note: this is **not** the CLAUDE.md the agent ships into the worktrees it manages. The agent never writes `CLAUDE.md` or `.claude/` into project checkouts — those repos own their own context (see memory: *No agent files in worktrees*).

## What this project is

A Rust/Axum HTTP service that listens for GitLab + GitHub webhooks and runs the local `claude` CLI against the affected repository. Output is parsed from `--output-format stream-json` and posted back as an issue/MR/PR comment. A Vue 3 SPA on the same port shows live task status, captured stdout, branch diff, and pending operator approvals.

Single-operator deployment by design — there is no multi-tenancy. Bearer-token auth on `/api/*` is the only access control.

## Engineering rules (apply to every change in this repo)

- **KISS.** Prefer the most direct expression. No premature abstraction, no future-proofing scaffolds, no DI-flavored indirection where a plain function works. Three similar lines beats a clever helper.
- **DRY.** If the same logic is starting to appear in two places, extract it — but only after the second occurrence, not before.
- **File size cap: 400 lines.** When a `.rs`/`.vue`/`.ts` file crosses 400 lines, split it along a natural seam (per-route handlers, per-trigger workflows, per-component slot). `src/jobs/store.rs` and `src/jobs/runner.rs` already exceed this by a wide margin — split them as soon as a non-trivial change lands; never grow a file that's already over.
- **Git workflow.** Rebase onto the latest `master` before starting work and never commit to `master` (derive a branch from the issue if none is given). Integrate fast-forward only — no merge commits. Commit/task messages use the **What / Why / How** format. Never add a `Co-Authored-By` trailer. See the `git` agent (`.claude/agents/git.md`).
- **Specialized agents.** `.claude/agents/` holds the subagents for working in this repo: `backend` (Rust, with unit + integration tests), `frontend` (Vue/TS), `git` (workflow above), and `review` (security + performance). Delegate stack-specific work to them.
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
src/jobs/runner.rs                clone/fetch, write the backend's worktree
    │   + src/agent (AgentBackend) files (Claude: .claude/settings.local.json),
    │                             spawn the backend as a long-lived interactive
    │                             session (`claude --input-format stream-json
    │                             --output-format stream-json`), pump operator
    │                             messages into its stdin, stream events into the
    │                             live hub, enforce token budget, and at session
    │                             end push changes + post result via GitProvider
    │
    ├─ stdout events ──► src/jobs/hub.rs (LiveSessions): per-task broadcast +
    │                    batch-persist to the task_events table; fan out over
    │                    GET /ws/tasks/{id} ◄──► Operator (SPA) chat/stop/redefine
    │
    │ PreToolUse hook (Bash/AskUserQuestion)
    ▼
POST /internal/authcheck          src/auth/handlers.rs — loopback-only;
    │                             allowlist match OR open auth_requests row,
    │                             block on AuthWaiter notify, and publish the
    │                             pending approval to the task's live hub
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
| `src/jobs/runner.rs` | `run_job` — spawns the agent backend as an interactive stream-json session, pumps operator messages into stdin, streams events to the hub, enforces token budget; at session end pushes commits + posts the result note |
| `src/jobs/hub.rs` | `LiveSessions` — per-task event hub: monotonic `seq`, `broadcast` fan-out to WS clients, batch-persist (every 100) to the `task_events` table, and the `mpsc` back-channel to the running agent's stdin |
| `src/jobs/prompt.rs` | `build_prompt` — per-trigger prompt text (split out of runner) |
| `src/jobs/stream.rs` | `stream_into_entry` — pumps a child pipe into the live log + publishes each stdout event to the hub, sniffing session id / output tokens via the backend |
| `src/agent/mod.rs` | `AgentBackend` trait + `WorktreeFile` — abstracts the coding-agent CLI (invocation, stdin message encoding, config/hook files, output parsing). Sync methods; the runner does the fs writes |
| `src/agent/claude.rs` | `ClaudeCode` — the only backend today (hardcoded in `run_job`); drives `claude` as an interactive stream-json session and parses the `result` event. Unit-tested |
| `src/ws/mod.rs` | `GET /ws/tasks/{id}` handler — token (`?token=`) checked in-handler; subscribes to the hub, streams `Envelope` frames, routes inbound chat/redefine/stop to the agent stdin |
| `src/jobs/registry.rs` | `RunningTasks` — abort handles by task id |
| `src/jobs/output_log.rs` | in-memory stdout/stderr ring — kept only for the final result parse + stderr error tail (no longer served over HTTP) |
| `src/workspace/mod.rs` | filesystem layout: `<base>/<service_slug>/<project_slug>/<branch_slug>/` |
| `src/workspace/git.rs` | `clone_or_fetch` |
| `src/workspace/lock.rs` | per-branch advisory file lock |
| `src/workspace/layout.rs` | `slugify` |
| `src/provider/mod.rs` | `GitProvider` trait + `NoteTarget` |
| `src/provider/registry.rs` | `ProviderRegistry` — per-service `Arc<dyn GitProvider>` cache, kept in sync with `git_services` table |
| `src/provider/{gitlab,github}/` | provider impls: REST calls for posting notes / approving / reading comments |
| `src/git_service/store.rs` | CRUD for the `git_services` table |
| `src/project/store.rs` | projects + project_branches tables, allowed_operations config |
| `src/api/*.rs` | HTTP handlers under `/api/` — tasks, projects, git_services, auth_requests |
| `src/auth/mod.rs` | `token_ok` — shared constant-time token check used by the bearer middleware (header) and the WS handler (query param) |
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

- `tasks` — one row per agent run; `status` is one of `pending|running|completed|failed|killed`. `pending_message` carries a queued follow-up for the resume path
- `task_events` — durable agent event stream; one row per event, PK `(task_id, seq)`, append-only, batch-inserted (100 at a time) by the live hub, cascade-deleted with the task
- `task_results` — final cost / turns / tokens / result text; one-to-one with tasks
- `projects` — discovered repos, per-project `allowed_operations` glob list
- `project_branches` — branches the agent has touched, with `issue_iid` / `pr_iid` linkage and status
- `auth_requests` — operator-approval items raised from the authcheck hook
- `git_services` — provider config: kind, base URL, bot username, PAT, webhook secret

### HTTP surface

Bearer-auth gates `/api/*` (and the SPA, when `API_BEARER_TOKEN` is set). `/webhook/*`, `/health`, `/internal/authcheck`, and `/ws/*` are outside that middleware; the authcheck endpoint is additionally restricted to loopback callers, and the WS handler validates the token from its `?token=` query param in-handler.

| Method | Path | Notes |
|---|---|---|
| `POST` | `/webhook/gitlab/{slug}` | `X-Gitlab-Token` = service `webhook_secret` |
| `POST` | `/webhook/github/{slug}` | `X-Hub-Signature-256` HMAC-SHA256 |
| `POST` | `/internal/authcheck` | loopback only; called by the PreToolUse hook |
| `GET` | `/ws/tasks/{id}` | WebSocket live stream; auth via `?token=`. Outbound `Envelope` frames (`event`/`auth_request`/`status`); inbound `{kind: chat\|redefine\|stop}` routed to the agent stdin |
| `GET` | `/api/tasks` | optional `?status=` |
| `POST` | `/api/tasks` | operator-driven dispatch: `{ project_id, trigger: TriggerReason }` → pending task (use when the webhook missed/was filtered) |
| `GET` | `/api/tasks/stats` | time spent per `?group_by=project\|service\|branch\|trigger_type` within `?from=`/`?to=` (default last 30d). Running tasks counted as `now - started_at`. |
| `GET`/`PATCH`/`DELETE` | `/api/tasks/{id}` | detail + result; PATCH edits a **pending** task's input fields (`branch`, `default_branch`) — run-managed fields are not editable and the branch may not equal the default branch; DELETE force-kills if running |
| `POST` | `/api/tasks/{id}/confirm` | pending → running |
| `POST` | `/api/tasks/{id}/retry` | clone the task as a new row |
| `POST` | `/api/tasks/{id}/kill` | SIGKILL; preserves session_id for Resume |
| `POST` | `/api/tasks/{id}/continue` | resume via `claude -r <session_id>` |
| `POST` | `/api/tasks/{id}/message` | queue a follow-up prompt for the resume path (used when the task is **not** live; live chat goes over the WS) |
| `GET` | `/api/tasks/{id}/diff` | `git diff origin/<default_branch>` of the task's worktree (+ untracked listing) |
| `GET` | `/api/tasks/{id}/events` | persisted agent events from `task_events` ordered by `seq`; seeds the SPA timeline before the WS streams live frames |
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
        ├── <branch_slug>.lock           # advisory file lock per branch
        ├── <branch_slug>/               # one git worktree per branch
        │   ├── .git/
        │   └── .claude/settings.local.json   # bypassPermissions + PreToolUse hook
        └── …
```

`slugify` lower-cases and replaces non-alphanumerics with `__`. Each task confirms the worktree exists (clone or fetch+reset), writes `settings.local.json`, then runs `claude --print [--resume <sid>] --input-format stream-json --output-format stream-json --verbose --replay-user-messages`. The process is **long-lived**: the initial prompt and every operator message are written to its stdin as `{"type":"user",…}` lines; it stays `running` (one `result` event per turn) until the operator sends **stop** (the hub drops the stdin sender → EOF → graceful exit) or **pause** (SIGKILL). At session end the runner pushes commits and posts the result note.

**Branch selection (a task never runs on the default branch).** `TaskStore::create_task` derives and persists `tasks.branch`: MR triggers reuse the MR's `source_branch`; an `Issue` trigger derives `<iid>-<slug(title)>` (e.g. `42-fix-login-button`); an `IssueComment` reuses the branch the original issue task recorded (`find_branch_for_issue`), falling back to bare `<iid>`. `workspace::git::clone_or_fetch(path, url, branch, default_branch)` checks out `origin/<branch>` if it exists remotely, otherwise creates the branch from `origin/<default_branch>` (`git checkout -f -B`); `push_changes` uses `git push -u origin HEAD` so a fresh branch gets its upstream. `run_job` hard-`bail!`s if the resolved branch equals the default branch.

The spawned `claude` inherits `CLAUDE_TASK_ID`, `AGENT_PORT`, and a provider-scoped PAT env var so `gh`/`glab` inside the worktree authenticate against the same token used for clone + note posting: `GH_TOKEN` for GitHub services, `GITLAB_TOKEN` for GitLab services.

### How an operator approval works

1. Claude tries to run a `Bash` or `AskUserQuestion`.
2. The PreToolUse hook (`defaults/.claude/hooks/authcheck.sh`) POSTs the command to `http://127.0.0.1:<port>/internal/authcheck` with the task's `CLAUDE_TASK_ID`.
3. The handler matches the command against the project's `allowed_operations` glob list (`auth/operations.rs`). On hit it returns `allowed:true` immediately.
4. On miss (or for `AskUserQuestion`), it creates an `auth_requests` row, **publishes it to the task's live hub** (so the task page shows the pending approval instantly over the WS), and parks on `AuthWaiter.register(id).notified()` for up to `OPERATOR_TIMEOUT_SECS` (600s).
5. The operator resolves via `POST /api/auth_requests/{id}/resolve`. The store wakes the waiter and publishes the resolution to the hub; the hook gets `{allowed, reply, reason}` and the Claude Code process continues.

> The hook remains the *decision* mechanism (it's the only documented way to gate a tool on a human). A follow-up issue tracks replacing it with Claude Code's stream-json **control protocol** (`can_use_tool` over the stdin/stdout we already own), which would delete the hook script, the `/internal/authcheck` loopback, and `AuthWaiter`.

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
- **Concurrency:** Tokio. Per-branch worktree setup uses `Workspace::lock_branch` (in-process `Mutex` + cross-process advisory file lock); tasks on different branches of the same project run concurrently. `confirm_task` blocks only when another task on the **same project+branch** is already running. Per-task work is just owned values.
- **SeaORM:** entity Model is the read shape; mutations go through `ActiveModel` + `Set(...)`. Cross-row consistency relies on individual statements being short — no explicit transactions today.
- **Idempotence:** dedupe at the event level via `seen_events: HashSet<String>` in `TaskStore`, keyed by `TriggerReason::event_id`. The set is in-memory; restarts re-deliver, but `created_at + trigger_data` makes duplicates easy to spot.
- **Bot-comment marker:** every comment posted via `GitProvider::post_note` gets `BOT_NOTE_MARKER` (`<!-- agent -->`) appended. The provider-side webhook normalizers drop incoming notes that contain the marker, so the bot never reacts to its own posts. This is the loop guard — the dispatcher no longer compares actor to `bot_username`, which means a same-account operator/bot setup still works.
- **Comments:** rule above; current code follows it inconsistently — bring new edits into line, don't write new "what" comments.

## Memory rules (for future Claude sessions)

- Clone repos over SSH only — see memory: *SSH only for git clones*.
- Never write `.claude/` or `CLAUDE.md` into the *project* worktrees the agent manages — see memory: *No agent files in worktrees*. (This `CLAUDE.md` is the agent's own, at the agent repo root — that is allowed.)
