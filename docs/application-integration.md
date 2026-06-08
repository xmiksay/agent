# Application integration — GitHub App (#9) & GitLab OAuth app (#10)

API exploration and the implementation plan for moving a `git_service` off a
static Personal Access Token (PAT) and onto a first-class **application** install.
This is the reference for issues [#9](https://github.com/xmiksay/agent/issues/9)
(GitHub App) and [#10](https://github.com/xmiksay/agent/issues/10) (GitLab OAuth
application). Issue [#15](https://github.com/xmiksay/agent/issues/15) lays the
groundwork described under **What is already prepared**; neither flow is wired
yet.

## Why move off PATs

A PAT is tied to a human account, carries that human's full scope, and never
expires on its own. An application install scopes access to exactly the repos it
is granted, issues short-lived tokens, and survives the owner leaving — the
right shape for a bot that comments on issues/MRs and pushes branches.

## What is already prepared (#15)

- **Schema** (`git_services`, migration `…_000018_add_git_service_app_auth`):
  `auth_kind TEXT NOT NULL DEFAULT 'pat'` plus nullable app credential columns —
  `app_id`, `app_installation_id`, `app_private_key`, `app_client_secret`,
  `app_refresh_token`. Existing rows keep working unchanged (`pat`).
- **Model** (`git_service::store`): `AuthKind { Pat, App }`, the new columns on
  `GitService`/`NewGitService`/`UpdateGitService`, and
  `GitService::credentials() -> ServiceCredentials` which validates that an
  `app` service has every column its provider needs.
- **`ServiceCredentials`** enum: `Pat`, `GitHubApp { app_id, private_key,
  installation_id }`, `GitLabOAuth { client_id, client_secret, refresh_token }`.
- **The seam** (`provider::credentials::resolve_token`): the *single* place that
  turns a credential into a usable access token. Every consumer — both REST
  clients (`post_note`) and the runner (`GH_TOKEN`/`GITLAB_TOKEN`) — already
  calls it. Today it returns the PAT and `bail!`s for the app variants. **The #9
  / #10 work is almost entirely inside this one function** (plus a token cache).
- **API/UI**: `auth_kind` and the non-secret `app_id`/`app_installation_id` are
  surfaced read-only; app secrets are write-only like `token`.

Column → credential mapping:

| Column | GitHub App (#9) | GitLab OAuth app (#10) |
|---|---|---|
| `app_id` | App ID (the numeric/client id used as JWT `iss`) | OAuth application ID (client id) |
| `app_installation_id` | Installation ID | — |
| `app_private_key` | App private key (PEM) | — |
| `app_client_secret` | — | OAuth application secret |
| `app_refresh_token` | — | Refresh token from the authorize step |

## GitHub App (#9)

### Concepts
A GitHub App is registered once (org or user owned). It has an **App ID**, one or
more **private keys** (PEM), a **webhook secret**, and a set of **permissions**
(e.g. Issues: read/write, Pull requests: read/write, Contents: read/write). A
user/org then **installs** it on selected repos, producing an **installation**
with its own numeric **installation ID**.

### Auth flow (two-step, all REST)
1. **App JWT** — sign a short-lived JWT (RS256) with the app private key:
   `iss` = App ID, `iat` = now-60s, `exp` ≤ now+10min. Used only for app-level
   endpoints.
2. **Installation access token** —
   `POST {api_base}/app/installations/{installation_id}/access_tokens`
   with `Authorization: Bearer <app_jwt>`. Returns
   `{ "token": "ghs_…", "expires_at": "…", "permissions": {…} }`. The token is
   valid **~1 hour** and is scoped to that installation.
3. Use the installation token exactly where the PAT is used today:
   - REST: `Authorization: Bearer ghs_…` (already how `GitHubClient::post_note`
     sends it).
   - git over HTTPS: clone/push as `https://x-access-token:<token>@github.com/…`.
     (Note: the agent clones over **SSH** today — App auth implies an HTTPS clone
     URL, so #9 must also thread an HTTPS `git_url` for app services.)

### Discovering the installation ID
Every webhook from an installed App includes `installation.id` in the payload —
the cheapest source. Alternatively `GET /repos/{owner}/{repo}/installation`
(with the app JWT) resolves a repo to its installation. For #15 we store it
explicitly in `app_installation_id`.

### GitHub Enterprise Server
`api_base` already drives the REST host (`base_url`), so GHES works by pointing
it at `https://ghes.example.com/api/v3`.

### What #9 must implement
- A JWT signer (RS256 over the stored PEM) + the access-token exchange in
  `resolve_token` for the `GitHubApp` variant.
- An in-memory token cache keyed by `service_id`, refreshing ~5 min before
  `expires_at` (the clients call `resolve_token` per request, so caching there is
  the only change they need).
- HTTPS clone URL handling for app-authed services.
- Crates: a JWT lib (e.g. `jsonwebtoken`) — not yet a dependency.

## GitLab application (#10)

### Concepts
GitLab has no exact GitHub-App equivalent. The closest first-class "application"
is an **OAuth 2.0 application** (instance/group/user owned) with an
**Application ID** (client id) and **Secret**. There is no per-repo installation
token; access is on behalf of the authorizing identity (ideally a dedicated bot
user or **service account**).

### Auth flow (OAuth2, Authorization Code)
1. Register the application (redirect URI, scopes — `api` covers notes + repo;
   narrower `read_api`+specific scopes if posting only).
2. One-time authorize → exchange the code at `POST {base_url}/oauth/token`
   (`grant_type=authorization_code`) for `{ access_token, refresh_token,
   expires_in }`. The **refresh token** is the durable credential we store in
   `app_refresh_token`.
3. **Refresh** when the access token expires (default 2h):
   `POST {base_url}/oauth/token` with `grant_type=refresh_token`,
   `client_id`, `client_secret`, `refresh_token`. With refresh-token rotation
   (default on modern GitLab) the response carries a **new** refresh token —
   `resolve_token` must persist it back to `app_refresh_token`, so #10 needs a
   store write on refresh, not just an in-memory cache.
4. Use the access token:
   - REST: `Authorization: Bearer <access_token>` (today `GitLabClient` sends
     `PRIVATE-TOKEN`; both are accepted by the GitLab API, but Bearer is correct
     for OAuth tokens).
   - git over HTTPS: `https://oauth2:<access_token>@gitlab.com/…`.

### Alternatives considered
- **Group/Project access tokens** — simpler (a bearer token with an expiry, no
  refresh dance), but they are not an "application" and expire ≤1y, needing
  manual rotation. Good fallback; doesn't need #10's OAuth machinery.
- **Service account + PAT** — works today via the existing `pat` flow; the
  cleanest interim option for self-hosted GitLab.

### What #10 must implement
- The refresh-token exchange in `resolve_token` for the `GitLabOAuth` variant,
  **including writing the rotated refresh token back to the store** (so this
  variant needs a `GitServiceStore` handle, unlike the GitHub path).
- Switch `GitLabClient` from `PRIVATE-TOKEN` to `Authorization: Bearer` when the
  service is app-authed.
- HTTPS clone URL handling.
- A one-time authorize step (out of band, or a small operator UI flow) to seed
  `app_refresh_token`.

## Token-cache shape (both)

`resolve_token` is async and called per request precisely so it can hide a cache:

```
service_id → { token, expires_at }
```

GitHub: refresh purely in memory (the installation token is re-mintable from the
PEM at will). GitLab: refresh must also persist the rotated `app_refresh_token`.
A `RwLock<HashMap<Uuid, CachedToken>>` alongside the registry is enough at
single-operator scale.

## Not in scope for #15

No JWT/OAuth code, no new crates, no clone-over-HTTPS, no token cache, no UI
form for app credentials. #15 is the schema + the seam only.
