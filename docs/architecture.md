# agent — architecture

Deep reference for the agent service. The project brief [`.claude/CLAUDE.md`](../.claude/CLAUDE.md) carries the engineering rules and points here. **Keep this file current:** when a change adds/removes/renames a module, route, entity, env var, or the way two modules talk to each other, edit the relevant section here in the same change.

## Flow

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

## Module map

| Path | Responsibility |
|---|---|
| `src/lib.rs` | crate library root: `pub mod` re-exports + the shared `AppState` (so `tests/` can link against the crate). `main.rs` is a thin binary over it |
| `src/main.rs` | thin binary: builds `AppState`, runs migrations, registers routes, starts axum |
| `src/config.rs` | env → `Config` struct |
| `src/webhook/gitlab.rs`, `src/webhook/github.rs` | per-provider HTTP handlers: HMAC verify, payload → `NormalizedEvent`, hand to `dispatch` |
| `src/webhook/normalized.rs` | provider-agnostic event shape (`EventKind`) |
| `src/webhook/dispatch.rs` | event → `TriggerReason`, dedupe, project upsert, task create. A comment on an issue/MR with an existing agent is **delivered as a message** to that task (one agent/session per issue/MR), not a new task. Issue/MR close stops the branch's live agent before reclaiming the worktree |
| `src/webhook/types.rs` | provider-specific payload structs |
| `src/jobs/types.rs` | `TriggerReason`, `ClaudeOutput` |
| `src/jobs/store.rs` | `TaskStore` core — the run loop (`confirm_task` + branch-conflict guard), `update_task` (PATCH), hub handle. Lifecycle/CRUD/query helpers live in the sibling modules below (store.rs was split at the 400-line cap) |
| `src/jobs/lifecycle.rs` | `derive_agent_state` (read-time overlay: `is_running` → `running`, `is_warm` → `warm`, else the durable `cold\|pending\|failed`), state-name constants, the `migrate_status` backfill mapping (testable), and the durable-state transition helpers on `TaskStore` — `recover_orphans`, `set_states`, `finish_task(agent_state, task_state, note)`, `note_task_result`, `publish_state` |
| `src/jobs/control.rs` | `TaskStore` operator controls — `kill_task` (Pause → durable `cold` / `task_state` `working_on` + "paused by operator" note), `delete_task`, `continue_task` (Resume), `push_message`, `clear_pending_message` |
| `src/jobs/create.rs` | `TaskStore::create_task` (seeds `agent_state=cold` / `task_state=pending`), `retry_task`, issue-branch naming |
| `src/jobs/queries.rs` | `TaskStore` reads — `list_tasks` (filters by `task_state` in SQL), `get_task`, `task_events`, `branch_diff`, `find_resumable_task_for_branch` |
| `src/jobs/runner.rs` | `run_job` — spawns the interactive stream-json session and runs the **turn loop**: per turn it takes a concurrency permit (`hub.mark_running` + `task_state → working_on`), forwards one operator message to the stdin writer task, waits for (and captures) that turn's `result` event, releases the permit (`hub.mark_idle` + `task_state → completed`, durable `cold`, no `finished_at` so the task can resume), finalizes from the captured event, then idles (warm, holding no slot) until the next message or a graceful close. Also owns the dedicated stdin writer task (so control responses reach the child mid-turn) and the permission consumer that spawns `handle_permission` per request. At session end it maps the claude child exit code to **`(agent_state, task_state, note)`** — exit 0 → `(cold, completed, —)`, non-zero → `(failed, failed, exit detail)`, budget-kill → `(failed, failed, "killed: token budget")` (operator Pause aborts the runner before this point; a graceful Stop makes claude exit 0 → completed) |
| `src/jobs/turn.rs` | `finalize_turn` — per-turn bookkeeping: takes the turn's `result` event (captured off the stream), parses it, persists it, pushes commits, and posts a reply note "on demand" (only when commits landed or the turn errored) |
| `src/jobs/hub.rs` | `LiveSessions` — per-task event hub: monotonic `seq`, `broadcast` fan-out to WS clients, and batch-persist (every 100) to the `task_events` table of **every** frame kind — agent events, `auth_request`, and `status` (each consumes a seq and is persisted, not just events). Plus two back-channels to the agent — `stdin` (operator messages, drained one per turn for pacing) and `control` (`respond_permission`, control responses written immediately). Plus per-channel in-memory liveness flags — `is_warm` (stdin sender present) and `is_running` (an `AtomicBool` set by `mark_running`/`mark_idle` around the active turn); these two feed `derive_agent_state` (warm/running are never persisted). Plus `send_to_agent`/`stop` for routing operator messages to a live session |
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
| `src/provider/credentials.rs` | `resolve_token(&ServiceCredentials)` — the single seam that turns a service's stored auth into a usable access token (REST + the agent's `GH_TOKEN`/`GITLAB_TOKEN`). Only `Pat` is wired; **GitHub App (#9) / GitLab OAuth app (#10) minting lands here**. Unit-tested |
| `src/provider/registry.rs` | `ProviderRegistry` — per-service `Arc<dyn GitProvider>` cache, kept in sync with `git_services` table. `build_client` resolves `GitService::credentials()`; a service whose `app` columns are incomplete is skipped on reload (warn) |
| `src/provider/{gitlab,github}/` | provider impls: REST calls for posting notes / approving / reading comments. Clients hold a `ServiceCredentials` and call `resolve_token` per request (so an app token can later be refreshed on expiry) |
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

## Database

PostgreSQL via SeaORM. Migrations live in `migration/src/` and run automatically on startup (`migration::Migrator::up(&db, None)` in `main.rs`). Adding a column means a new migration file; never mutate an old one.

Tables (current set, see migration files for canonical schemas):

- `tasks` — one row per agent run, with **two orthogonal state axes**. `task_state` (persisted, operator-owned lifecycle: `pending|working_on|completed|failed`) is auto-advanced by the runner but freely PATCH-overridable. `agent_state` is **derived at read time** by `derive_agent_state` (`cold|warm|pending|running|failed`) — its persisted backing column narrows to `cold|pending|failed`, and `warm`/`running` are overlaid from the hub's in-memory liveness flags, never stored. There is no `killed` state — the disposition reason (paused / budget / orphan) is recorded as `task_results` text instead. `pending_message` carries a queued follow-up for the resume path
- `task_events` — durable hub-frame stream; one row per frame of any kind (agent event, `auth_request`, `status`) distinguished by the `kind` column, PK `(task_id, seq)`, each frame consuming a unique seq, append-only, batch-inserted (100 at a time) by the live hub, cascade-deleted with the task
- `task_results` — final cost / turns / tokens / result text; one-to-one with tasks
- `projects` — discovered repos, per-project `allowed_operations` glob list and `env_file` (a `.env`-style minijinja template injected as env vars at agent spawn)
- `project_branches` — branches the agent has touched, with `issue_iid` / `pr_iid` linkage and status
- `auth_requests` — operator-approval items raised by the in-process permission handler
- `git_services` — provider config: kind, base URL, bot username, PAT, webhook secret, `autofire` (when true, a newly-created task from this service's webhook is auto-confirmed — started running immediately instead of left pending for a manual confirm). `auth_kind` (`pat`\|`app`, default `pat`) selects the credential flow; the `app_*` columns (`app_id`, `app_installation_id`, `app_private_key`, `app_client_secret`, `app_refresh_token`) are **groundwork for GitHub App (#9) / GitLab OAuth application (#10)** — stored and validated, but token minting is not implemented yet (see `provider::credentials` and [`docs/application-integration.md`](application-integration.md))

## HTTP surface

Bearer-auth gates `/api/*` (and the SPA, when `API_BEARER_TOKEN` is set). `/webhook/*`, `/health`, and `/ws` are outside that middleware; the `/ws` handler authenticates in-band (the client's first frame carries the token), so it never lands in URLs/proxy logs.

| Method | Path | Notes |
|---|---|---|
| `POST` | `/webhook/gitlab/{slug}` | `X-Gitlab-Token` = service `webhook_secret` |
| `POST` | `/webhook/github/{slug}` | `X-Hub-Signature-256` HMAC-SHA256 |
| `GET` | `/ws` | **Single app-wide** WebSocket live stream (multiplexes all tasks). In-band auth (first frame = token). Outbound `Envelope` frames (`task_id`/`event`\|`auth_request`\|`status`); inbound `{kind: chat\|redefine\|stop, task_id}` routed to the agent stdin |
| `GET` | `/api/tasks` | optional `?task_state=` (SQL `WHERE` on the persisted column) and `?agent_state=` (derived per task, filtered in-memory — acceptable at single-operator scale). Each task emits both `task_state` and the derived `agent_state`; no `live` boolean |
| `POST` | `/api/tasks` | operator-driven dispatch: `{ project_id, trigger: TriggerReason }` → pending task (use when the webhook missed/was filtered) |
| `GET` | `/api/tasks/stats` | time spent per `?group_by=project\|service\|branch\|trigger_type` within `?from=`/`?to=` (default last 30d). Running tasks counted as `now - started_at`. |
| `GET`/`PATCH`/`DELETE` | `/api/tasks/{id}` | detail + result; PATCH sets `task_state` to any of `pending\|working_on\|completed\|failed` on **any** task (operator override of the auto-advance), and edits input fields (`branch`, `default_branch`) only while `task_state == pending` — the branch may not equal the default branch; DELETE force-kills if running |
| `POST` | `/api/tasks/{id}/confirm` | pending → queued (durable `agent_state` pending) |
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

## Workspace layout on disk

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

The process is **long-lived and turn-based**. The runner's turn loop, per turn: takes a `MAX_CONCURRENT_JOBS` permit (`hub.mark_running` → derived `agent_state` `running`, `task_state → working_on`), writes one operator message to stdin (the first turn uses the initial prompt), waits for that turn's `result`, **releases the permit** (`hub.mark_idle` → derived `agent_state` `warm`, `task_state → completed`), finalizes (push commits + reply-on-demand), then **idles warm** — the agent process stays alive but holds **no concurrency slot** (an idle agent is not a running agent). A new message (live via `hub.send_to_agent`, or a resume) wakes it into the next turn. The session ends on **stop** (hub drops the stdin sender → EOF → graceful exit), **pause** (SIGKILL), token-budget kill, or issue/MR close. `push_message` delivers to a warm agent first (`hub.is_warm`) and only resumes the session when there's no live agent — so a follow-up never spawns a second agent on the same branch.

**Branch selection (a task never runs on the default branch).** `TaskStore::create_task` derives and persists `tasks.branch`: MR triggers reuse the MR's `source_branch`; an `Issue` trigger derives `<iid>-<slug(title)>` (e.g. `42-fix-login-button`); an `IssueComment` reuses the branch the original issue task recorded (`find_branch_for_issue`), falling back to bare `<iid>`. **Comments delegate:** when a resumable task already exists for the issue/MR branch (`find_resumable_task_for_branch`), the dispatcher delivers the comment via `push_message` to that one task — continuing the same agent/session — instead of creating a fresh task; a new task is created only when there's no prior session. `workspace::git::clone_or_fetch(path, url, branch, default_branch)` clones on first use, fetches, then: if the worktree is **already on `branch`** it is left untouched (local commits + uncommitted work are preserved across runs — what makes resume-on-message safe); otherwise it checks out `origin/<branch>` if it exists remotely, else creates the branch from `origin/<default_branch>` (`git checkout -B`, no force-reset). `push_changes` uses `git push -u origin HEAD` so a fresh branch gets its upstream. `run_job` hard-`bail!`s if the resolved branch equals the default branch.

The spawned `claude` inherits a provider-scoped token env var so `gh`/`glab` inside the worktree authenticate against the same token used for clone + note posting: `GH_TOKEN` for GitHub services, `GITLAB_TOKEN` for GitLab services. The value comes from `provider::credentials::resolve_token(service.credentials()?)` — today the stored PAT, later a minted app/installation token. The project's `env_file` is rendered (minijinja, against runtime vars `branch`/`default_branch`/`url`/`project`/`service`/`task_id` — see `src/project/env.rs`) and applied **before** this reserved var, so a project env can never clobber the PAT.

## How an operator approval works

Tool gating runs entirely over the stream-json **control protocol** — the same stdin/stdout the runner already owns — so there is no hook script, no `/internal/authcheck` loopback, and no `CLAUDE_TASK_ID`/`AGENT_PORT`. `--permission-mode default --permission-prompt-tool stdio` makes the CLI emit a `can_use_tool` `control_request` on stdout for every non-trivially-safe tool and wait for a `control_response` on stdin.

1. The stdout pump (`src/jobs/stream.rs`) detects a `can_use_tool` line via `backend.parse_permission_request` and forwards it to the permission consumer instead of publishing it as a timeline event.
2. `handle_permission` (`src/jobs/permission.rs`) applies the policy: any tool other than `Bash`/`AskUserQuestion` (edits, reads) is **allowed immediately**, echoing the input back as `updatedInput`.
3. For `Bash`, the command is matched against the project's `allowed_operations` glob list (`auth/operations.rs`). On hit it allows immediately.
4. On miss (or for `AskUserQuestion`), it creates an `auth_requests` row, **publishes it to the task's live hub** (so the task page shows the pending approval instantly over the WS), and parks on `AuthWaiter.register(id).notified()` for up to `OPERATOR_TIMEOUT_SECS` (600s).
5. The operator resolves via `POST /api/auth_requests/{id}/resolve`. The store wakes the waiter and publishes the resolution to the hub; the handler encodes the decision (allow, or deny-with-message — `AskUserQuestion` is always a deny whose message the model reads as the answer) and `hub.respond_permission` writes the `control_response` straight to the agent's stdin, bypassing the per-turn pacing so a mid-turn prompt is answered without waiting for the turn to end.
