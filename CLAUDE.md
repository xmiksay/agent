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
src/jobs/runner.rs                clone/fetch, spawn the backend as a long-lived
    │                             interactive session (`claude --input-format
    │                             stream-json --output-format stream-json
    │                             --permission-mode default --permission-prompt-tool
    │                             stdio`), pump operator messages into its stdin,
    │                             stream events into the live hub, enforce token
    │                             budget, and at session end push changes + post
    │                             result via GitProvider
    │
    ├─ stdout events ──► src/jobs/hub.rs (LiveSessions): per-task broadcast +
    │                    batch-persist to the task_events table; fan out over
    │                    one app-wide GET /ws ◄──► Operator (SPA) chat/stop/redefine
    │
    │ can_use_tool control_request on stdout (Bash/AskUserQuestion/…)
    ▼
src/jobs/permission.rs            handle_permission — in-process; non-Bash tools
    │                             (edits/reads) auto-allowed, Bash matched against
    │                             the project allowlist; on miss / AskUserQuestion
    │                             open an auth_requests row, block on AuthWaiter
    │                             notify, publish to the live hub, then write a
    │                             control_response back to the agent's stdin
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
| `src/webhook/dispatch.rs` | event → `TriggerReason`, dedupe, project upsert, task create. A comment on an issue/MR with an existing agent is **delivered as a message** to that task (one agent/session per issue/MR), not a new task. Issue/MR close stops the branch's live agent before reclaiming the worktree |
| `src/webhook/types.rs` | provider-specific payload structs |
| `src/jobs/types.rs` | `TriggerReason`, `ClaudeOutput` |
| `src/jobs/store.rs` | `TaskStore` — task CRUD, run loop, kill/continue/retry/push_message, branch_diff. **Over 500 lines — split before adding new methods.** |
| `src/jobs/runner.rs` | `run_job` — spawns the interactive stream-json session and runs the **turn loop**: per turn it takes a concurrency permit, forwards one operator message to the stdin writer task, waits for (and captures) that turn's `result` event, releases the permit, finalizes from the captured event, then idles (warm, holding no slot) until the next message or a graceful close. Also owns the dedicated stdin writer task (so control responses reach the child mid-turn) and the permission consumer that spawns `handle_permission` per request. At session end it derives the **final task status from the claude child exit code** — exit 0 → `completed`, non-zero → `failed`, budget-kill → `killed` (operator Pause aborts the runner before this point, so the exit-code path only covers natural exits / crashes; a graceful Stop makes claude exit 0 → completed) |
| `src/jobs/turn.rs` | `finalize_turn` — per-turn bookkeeping: takes the turn's `result` event (captured off the stream), parses it, persists it, pushes commits, and posts a reply note "on demand" (only when commits landed or the turn errored) |
| `src/jobs/hub.rs` | `LiveSessions` — per-task event hub: monotonic `seq`, `broadcast` fan-out to WS clients, and batch-persist (every 100) to the `task_events` table of **every** frame kind — agent events, `auth_request`, and `status` (each consumes a seq and is persisted, not just events). Plus two back-channels to the agent — `stdin` (operator messages, drained one per turn for pacing) and `control` (`respond_permission`, control responses written immediately). Plus `is_warm`/`send_to_agent`/`stop` for routing operator messages to a live session |
| `src/jobs/prompt.rs` | `build_prompt` — per-trigger prompt text (split out of runner) |
| `src/jobs/stream.rs` | `stream_into_entry` — pumps a child pipe: stdout lines are published to the hub as agent events (no in-memory buffering — `task_events` is the durable record), sniffing session id / output tokens via the backend, routing `can_use_tool` control requests to the permission handler instead of publishing them, and forwarding the turn's `result` event to the runner; stderr is drained to EOF and logged per-line at `debug` |
| `src/jobs/permission.rs` | `handle_permission` — applies the allowlist-or-operator policy to one `can_use_tool` request and writes the `control_response` back via `hub.respond_permission`. Non-Bash/AskUserQuestion tools are auto-allowed; the 600s operator timeout lives here. Unit-tested |
| `src/agent/mod.rs` | `AgentBackend` trait + `PermissionRequest`/`PermissionDecision` — abstracts the coding-agent CLI (invocation, stdin message encoding, control-protocol parse/encode, output parsing). Sync, pure methods |
| `src/agent/claude.rs` | `ClaudeCode` — the only backend today (hardcoded in `run_job`); drives `claude` as an interactive stream-json session, parses the `result` event, and parses/encodes the `can_use_tool` control protocol. Unit-tested |
| `src/ws/mod.rs` | `GET /ws` — **one** process-wide socket per browser. In-band auth (the client's first frame is its token); subscribes to the hub's `all` stream and forwards every task's `Envelope` frames; routes inbound `{kind, task_id, …}` operator messages to the agent stdin; 30s keepalive ping |
| `src/jobs/registry.rs` | `RunningTasks` — abort handles by task id |
| `src/workspace/mod.rs` | filesystem layout: `<base>/<service_slug>/<project_slug>/<branch_slug>/` |
| `src/workspace/git.rs` | `clone_or_fetch` |
| `src/workspace/lock.rs` | per-branch advisory file lock |
| `src/workspace/layout.rs` | `slugify` |
| `src/provider/mod.rs` | `GitProvider` trait + `NoteTarget` |
| `src/provider/registry.rs` | `ProviderRegistry` — per-service `Arc<dyn GitProvider>` cache, kept in sync with `git_services` table |
| `src/provider/{gitlab,github}/` | provider impls: REST calls for posting notes / approving / reading comments |
| `src/git_service/store.rs` | CRUD for the `git_services` table |
| `src/project/store.rs` | projects + project_branches tables, allowed_operations + env_file config |
| `src/project/env.rs` | renders a project's `env_file` (a minijinja template) against the task's runtime vars and parses the result into `(key, value)` pairs injected at spawn. Unit-tested |
| `src/api/*.rs` | HTTP handlers under `/api/` — tasks, projects, git_services, auth_requests |
| `src/auth/mod.rs` | `token_ok` — shared constant-time token check used by the bearer middleware (header) and the WS handler (first-frame token) |
| `src/auth/middleware.rs` | bearer-token check for `/api/*` |
| `src/auth/store.rs` | `auth_requests` CRUD + status enum |
| `src/auth/waiter.rs` | in-process `Notify` map; the permission handler parks here until the operator resolves |
| `src/auth/operations.rs` | glob matcher for allowlist evaluation |
| `src/entity/*.rs` | SeaORM `Model` structs (one per table) |
| `migration/src/*.rs` | SeaORM migrations — append-only, numbered `mYYYYMMDD_NNNNNN_*` |
| `src/spa.rs` | `rust-embed` handler — bakes `frontend/dist/` into the binary and serves it as the `/`-fallback (SPA paths fall through to `index.html`) |
| `frontend/` | Vue 3 + Vite + Pinia + Tailwind SPA — `npm run build` must run before `cargo build` so the bundle is on disk when the embed derive picks it up |
| `frontend/public/` | PWA assets copied verbatim into `dist/` by Vite: `manifest.webmanifest`, icons (`icon-192/512.png`, `maskable-512.png`, `apple-touch-icon.png`, `favicon.svg/ico`), and `sw.js` — a **network-first** service worker (cache is offline-only fallback) so an installed home-screen app always reflects the live site. Registered in prod by `frontend/src/pwa.ts`; served by the `/`-fallback (no bearer gate), so install works regardless of `API_BEARER_TOKEN` |

### Database

PostgreSQL via SeaORM. Migrations live in `migration/src/` and run automatically on startup (`migration::Migrator::up(&db, None)` in `main.rs`). Adding a column means a new migration file; never mutate an old one.

Tables (current set, see migration files for canonical schemas):

- `tasks` — one row per agent run; `status` is one of `pending|running|completed|failed|killed`. `pending_message` carries a queued follow-up for the resume path
- `task_events` — durable hub-frame stream; one row per frame of any kind (agent event, `auth_request`, `status`) distinguished by the `kind` column, PK `(task_id, seq)`, each frame consuming a unique seq, append-only, batch-inserted (100 at a time) by the live hub, cascade-deleted with the task
- `task_results` — final cost / turns / tokens / result text; one-to-one with tasks
- `projects` — discovered repos, per-project `allowed_operations` glob list and `env_file` (a `.env`-style minijinja template injected as env vars at agent spawn)
- `project_branches` — branches the agent has touched, with `issue_iid` / `pr_iid` linkage and status
- `auth_requests` — operator-approval items raised by the in-process permission handler
- `git_services` — provider config: kind, base URL, bot username, PAT, webhook secret, `autofire` (when true, a newly-created task from this service's webhook is auto-confirmed — started running immediately instead of left pending for a manual confirm)

### HTTP surface

Bearer-auth gates `/api/*` (and the SPA, when `API_BEARER_TOKEN` is set). `/webhook/*`, `/health`, and `/ws` are outside that middleware; the `/ws` handler authenticates in-band (the client's first frame carries the token), so it never lands in URLs/proxy logs.

| Method | Path | Notes |
|---|---|---|
| `POST` | `/webhook/gitlab/{slug}` | `X-Gitlab-Token` = service `webhook_secret` |
| `POST` | `/webhook/github/{slug}` | `X-Hub-Signature-256` HMAC-SHA256 |
| `GET` | `/ws` | **Single app-wide** WebSocket live stream (multiplexes all tasks). In-band auth (first frame = token). Outbound `Envelope` frames (`task_id`/`event`\|`auth_request`\|`status`); inbound `{kind: chat\|redefine\|stop, task_id}` routed to the agent stdin |
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
| `GET` | `/api/projects` / `GET /api/projects/{id}` / `PUT /api/projects/{id}/config` / `PUT /api/projects/{id}/env` / `GET /api/projects/{id}/branches` | `/env` sets the `.env` minijinja template |
| `GET`/`POST` | `/api/git_services` | tokens and webhook secrets are write-only on GET |
| `GET`/`PUT`/`DELETE` | `/api/git_services/{id}` | |
| `GET` | `/api/auth_requests` | optional `?status=`, `?task_id=` |
| `GET`/`POST` | `/api/auth_requests/{id}` / `/api/auth_requests/{id}/resolve` | unblocks the parked permission handler |
| `GET` | `/api/auth/check` | 204 probe used by the SPA to validate the token |
| `GET` | `/health` | 200 |

### Workspace layout on disk

```
$REPO_BASE_PATH/
└── <service_slug>/
    └── <project_slug>/
        ├── <branch_slug>.lock           # advisory file lock per branch
        ├── <branch_slug>/               # one git worktree per branch
        │   └── .git/
        └── …
```

`slugify` lower-cases and replaces non-alphanumerics with `__`. Each task confirms the worktree exists (clone/fetch, preserving an already-checked-out branch), then runs `claude --print [--resume <sid>] --input-format stream-json --output-format stream-json --verbose --replay-user-messages --permission-mode default --permission-prompt-tool stdio`. The agent's own config/hook files are **not** written into the worktree — permission behavior is a CLI flag, and tool gating is the in-process control-protocol handler.

The process is **long-lived and turn-based**. The runner's turn loop, per turn: takes a `MAX_CONCURRENT_JOBS` permit (status → `running`), writes one operator message to stdin (the first turn uses the initial prompt), waits for that turn's `result`, **releases the permit** (status → `completed`), finalizes (push commits + reply-on-demand), then **idles warm** — the agent process stays alive but holds **no concurrency slot** (an idle agent is not a running agent). A new message (live via `hub.send_to_agent`, or a resume) wakes it into the next turn. The session ends on **stop** (hub drops the stdin sender → EOF → graceful exit), **pause** (SIGKILL), token-budget kill, or issue/MR close. `push_message` delivers to a warm agent first (`hub.is_warm`) and only resumes the session when there's no live agent — so a follow-up never spawns a second agent on the same branch.

**Branch selection (a task never runs on the default branch).** `TaskStore::create_task` derives and persists `tasks.branch`: MR triggers reuse the MR's `source_branch`; an `Issue` trigger derives `<iid>-<slug(title)>` (e.g. `42-fix-login-button`); an `IssueComment` reuses the branch the original issue task recorded (`find_branch_for_issue`), falling back to bare `<iid>`. **Comments delegate:** when a resumable task already exists for the issue/MR branch (`find_resumable_task_for_branch`), the dispatcher delivers the comment via `push_message` to that one task — continuing the same agent/session — instead of creating a fresh task; a new task is created only when there's no prior session. `workspace::git::clone_or_fetch(path, url, branch, default_branch)` clones on first use, fetches, then: if the worktree is **already on `branch`** it is left untouched (local commits + uncommitted work are preserved across runs — what makes resume-on-message safe); otherwise it checks out `origin/<branch>` if it exists remotely, else creates the branch from `origin/<default_branch>` (`git checkout -B`, no force-reset). `push_changes` uses `git push -u origin HEAD` so a fresh branch gets its upstream. `run_job` hard-`bail!`s if the resolved branch equals the default branch.

The spawned `claude` inherits a provider-scoped PAT env var so `gh`/`glab` inside the worktree authenticate against the same token used for clone + note posting: `GH_TOKEN` for GitHub services, `GITLAB_TOKEN` for GitLab services. The project's `env_file` is rendered (minijinja, against runtime vars `branch`/`default_branch`/`url`/`project`/`service`/`task_id` — see `src/project/env.rs`) and applied **before** this reserved var, so a project env can never clobber the PAT.

### How an operator approval works

Tool gating runs entirely over the stream-json **control protocol** — the same stdin/stdout the runner already owns — so there is no hook script, no `/internal/authcheck` loopback, and no `CLAUDE_TASK_ID`/`AGENT_PORT`. `--permission-mode default --permission-prompt-tool stdio` makes the CLI emit a `can_use_tool` `control_request` on stdout for every non-trivially-safe tool and wait for a `control_response` on stdin.

1. The stdout pump (`src/jobs/stream.rs`) detects a `can_use_tool` line via `backend.parse_permission_request` and forwards it to the permission consumer instead of publishing it as a timeline event.
2. `handle_permission` (`src/jobs/permission.rs`) applies the policy: any tool other than `Bash`/`AskUserQuestion` (edits, reads) is **allowed immediately**, echoing the input back as `updatedInput`.
3. For `Bash`, the command is matched against the project's `allowed_operations` glob list (`auth/operations.rs`). On hit it allows immediately.
4. On miss (or for `AskUserQuestion`), it creates an `auth_requests` row, **publishes it to the task's live hub** (so the task page shows the pending approval instantly over the WS), and parks on `AuthWaiter.register(id).notified()` for up to `OPERATOR_TIMEOUT_SECS` (600s).
5. The operator resolves via `POST /api/auth_requests/{id}/resolve`. The store wakes the waiter and publishes the resolution to the hub; the handler encodes the decision (allow, or deny-with-message — `AskUserQuestion` is always a deny whose message the model reads as the answer) and `hub.respond_permission` writes the `control_response` straight to the agent's stdin, bypassing the per-turn pacing so a mid-turn prompt is answered without waiting for the turn to end.

## Configuration

Read by `src/config.rs::from_env`. Defaults in parentheses.

| Var | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | required | Postgres DSN |
| `REPO_BASE_PATH` | `/tmp/claude-jobs` | worktree base |
| `LISTEN_ADDR` | `0.0.0.0:3000` | bind address |
| `MAX_CONCURRENT_JOBS` | `3` | Tokio semaphore size — gates **actively-processing turns**, acquired/released per turn so idle warm agents hold no slot |
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
