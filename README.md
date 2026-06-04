# gitlab-claude-agent

A Rust/Axum service that listens for GitLab and GitHub webhooks, runs the `claude` CLI against the affected repo, and posts the result back as an issue/MR/PR comment.

## Setup

### 1. Configure the host

Copy `.env.example` to `.env` and set:

- `DATABASE_URL` — Postgres connection string
- `REPO_BASE_PATH` — where working trees live (`<base>/<service_slug>/<project_slug>/branches/<branch>`)
- `LISTEN_ADDR` — defaults to `0.0.0.0:3000`
- `API_BEARER_TOKEN` *(optional)* — protects `/api/*` and the SPA

GitLab/GitHub credentials are **not** in `.env` — they live in the `git_services` table and are managed via the admin UI.

### 2. Run

```bash
cargo run
cd frontend && npm install && npm run build
```

Visit `http://localhost:3000` and (if `API_BEARER_TOKEN` is set) paste the token.

### 3. Add a git service

Go to **Services → Add service** and fill in:

| Field            | Example                               |
|------------------|---------------------------------------|
| Kind             | `gitlab` (multiple allowed) / `github` (single) |
| Slug             | `personal`                            |
| Display name     | `Personal GitLab`                     |
| Base URL         | `https://gitlab.com`                  |
| Bot username     | `claude-bot`                          |
| Personal token   | `glpat-…` (PAT with `api`, `read_repository`, `write_repository`) |
| Webhook secret   | a random string                       |

After saving, the service detail page shows the **Webhook URL** to paste into GitLab/GitHub.

## Webhook endpoints

Per service:

```
POST  /webhook/gitlab/<slug>     # X-Gitlab-Token = the service's webhook_secret
POST  /webhook/github/<slug>     # X-Hub-Signature-256 HMAC-SHA256 of body, key = webhook_secret
```

`<slug>` is the value you entered when creating the service. Service-detail page renders the full URL for convenience.

### GitLab side

Project → **Settings → Webhooks → Add new webhook**:

- **URL** — `https://<your-agent-host>/webhook/gitlab/<slug>`
- **Secret token** — the `webhook_secret` you saved
- **Trigger** — Issues events, Comments, Merge request events (Confidential variants too if you want)
- **SSL verification** — on

You can register the same webhook at group level (`Settings → Webhooks` on the group) so it applies to every project beneath it.

To register via API instead of UI:

```bash
curl --request POST \
     --header "PRIVATE-TOKEN: $GITLAB_PAT" \
     --header "Content-Type: application/json" \
     --data '{
       "url": "https://your-agent.example.com/webhook/gitlab/personal",
       "token": "the-webhook-secret",
       "issues_events": true,
       "note_events": true,
       "merge_requests_events": true,
       "confidential_issues_events": true,
       "confidential_note_events": true,
       "enable_ssl_verification": true
     }' \
     "https://gitlab.com/api/v4/projects/<project_id>/hooks"
```

### GitHub side

Repo → **Settings → Webhooks → Add webhook**:

- **Payload URL** — `https://<your-agent-host>/webhook/github/<slug>`
- **Content type** — `application/json`
- **Secret** — the `webhook_secret` you saved
- **Events** — Issues, Issue comments, Pull requests, Pull request reviews

## How it triggers

| Event                                                  | Action |
|--------------------------------------------------------|--------|
| Issue assigned to the bot user                         | Implement, push branch, open MR/PR |
| MR/PR review with **changes requested**                | Fix and push |
| Any other MR/PR review                                 | Post a review note |
| Issue/MR/PR comment containing `@claude`               | Reply in-thread |
| Issue closed / MR/PR closed/merged                     | Release the checked-out branch |

The bot user is the `bot_username` configured on the git service — only events involving that user trigger work.

## API

All routes under `/api/*` require `Authorization: Bearer $API_BEARER_TOKEN` if that env var is set.

- `GET    /api/git_services` / `POST /api/git_services`
- `GET    /api/git_services/{id}` / `PUT` / `DELETE`
- `GET    /api/projects` / `GET /api/projects/{id}` / `PUT /api/projects/{id}/config`
- `GET    /api/tasks` / `GET /api/tasks/{id}` / `POST /api/tasks/{id}/confirm`
- `GET    /api/auth_requests` / `GET /api/auth_requests/{id}` / `POST /api/auth_requests/{id}/resolve`

Token and webhook secret are write-only — they're never returned by `GET /api/git_services`.
