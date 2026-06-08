# Application integration — GitHub App (#9) & GitLab bot identity (#10)

API exploration and the implementation plan for moving a `git_service` off the
operator's static Personal Access Token (PAT) and onto an **independent agent
identity**. This is the reference for issues
[#9](https://github.com/xmiksay/agent/issues/9) (GitHub App) and
[#10](https://github.com/xmiksay/agent/issues/10) (GitLab bot + Group/Project
Access Token). Issue [#15](https://github.com/xmiksay/agent/issues/15) laid the
`auth_kind`/`app_credentials` groundwork. **GitHub App (#9) is now wired** (see
*What #9 implemented* below and `docs/architecture.md` → *GitHub App auth*).

The two providers diverge: GitHub gets a first-class **App** install (#9, now
wired). GitLab gets a **bot/service account** authenticating with a
**Group/Project Access Token** (#10) — which is just a `pat`, so it needs no new
credential code. The earlier "GitLab OAuth application" framing was dropped (see
[Why a bot, not OAuth](#why-a-bot-not-oauth-10)).

## Why move off the operator's PAT

A personal PAT is tied to a human account, carries that human's full scope, and
never expires on its own. It also makes the agent act *as the operator* — the
opposite of an unbound agent. An independent identity (a GitHub App install, or a
GitLab bot/service account) scopes access to exactly the repos it is granted and
acts as itself when it comments and pushes.

## What is already prepared (#15)

- **Schema** (`git_services`, migration `…_000018_add_git_service_app_auth`):
  the credential is a **type + value** pair — `auth_kind TEXT NOT NULL DEFAULT
  'pat'` (type) and `app_credentials JSONB NULL` (value, the provider-specific
  secret bundle). One JSON column instead of a flat union of every provider's
  possible fields, so a new provider's app shape needs no migration. Existing
  rows keep working unchanged (`pat`).
- **Model** (`git_service::store`): `AuthKind { Pat, App }`, the typed
  `GitHubAppConfig` shape for the GitHub JSON, and
  `GitService::credentials() -> ServiceCredentials` which parses+validates
  `app_credentials` against the service's provider.
- **`ServiceCredentials`** enum: `Pat(String)`, `GitHubApp(GitHubAppConfig)`.
- **The seam** (`provider::credentials::resolve_token`): the *single* place that
  turns a credential into a usable access token. Every consumer — both REST
  clients (`post_note`) and the runner (`GH_TOKEN`/`GITLAB_TOKEN`) — already
  calls it. Today it returns the token for `Pat` and `bail!`s for `GitHubApp`.
  **The #9 work is almost entirely inside this one function** (plus a token
  cache). GitLab needs no work here — its bot token flows straight through `Pat`.
- **API/UI**: `auth_kind` is surfaced read-only; the whole `app_credentials`
  bundle is write-only like `token`/`webhook_secret`.

`app_credentials` JSON shape (when `auth_kind = 'app'`):

| Provider | JSON keys |
|---|---|
| GitHub App (#9) | `app_id` (App ID, the JWT `iss`), `private_key` (PEM), `installation_id` |

GitLab has no `app_credentials` shape — `auth_kind = 'app'` is rejected for
GitLab services.

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
     (Token-HTTPS transport landed in #22 — the App path threads the installation
     token through the same `HttpsAuth` credential helper.)

### Discovering the installation ID
Every webhook from an installed App includes `installation.id` in the payload —
the cheapest source. Alternatively `GET /repos/{owner}/{repo}/installation`
(with the app JWT) resolves a repo to its installation. For #15 we store it
explicitly as `app_credentials.installation_id`.

### GitHub Enterprise Server
`api_base` already drives the REST host (`base_url`), so GHES works by pointing
it at `https://ghes.example.com/api/v3`.

### What #9 implemented (`src/provider/github/app.rs`)
- A JWT signer (RS256 over the stored PEM, via `jsonwebtoken`) + the
  installation access-token exchange, driven from `resolve_token`'s `GitHubApp`
  arm. `api_base` is threaded into `GitHubAppConfig` from the service's
  `base_url`, so GHES works.
- An in-memory token cache keyed by `{api_base}#{installation_id}`, refreshing
  ~5 min before `expires_at` (the clients call `resolve_token` per request, so
  caching there is the only change they needed).
- HTTPS clone/push already works unchanged: token-HTTPS transport (#22) auths
  GitHub as `x-access-token:<token>`, and an installation token slots straight
  in.
- The **install flow**: `GET /api/git_services/{id}/github_app/install` resolves
  the App's install URL (`GET /app` → `html_url`, with the service id as
  `state`); `GET /github_app/callback` captures `installation_id` back into
  `app_credentials`. App-auth services **skip** per-repo `ensure_webhook` in
  favor of the App's single app-level webhook.
- `installation_id` is optional at create/validate time — blank until the
  install callback writes it; minting fails with a clear "not installed" error
  until then.

## GitLab bot identity (#10)

GitLab gets its own **independent identity** as a bot/service account
authenticating with a **Group (or Project) Access Token** — `auth_kind = 'pat'`,
so the existing `pat` path already carries it end to end (REST `post_note`, the
`GITLAB_TOKEN` the agent inherits, and the token-HTTPS git transport from #22).
This issue is about *provisioning* that identity, not building a new auth flow.

### Why a bot, not OAuth (#10)
GitLab's nearest "application" is an OAuth 2.0 app, but authorization-code OAuth
binds the token to the **human who authorizes it** — the agent would still act as
that person. A **Group/Project Access Token** instead mints a dedicated
non-human user (`group_NNN_bot_*` / `project_NNN_bot_*`) owned by the group, so
the agent is its own actor. It is also far simpler: a bearer token with an
expiry, no refresh dance, no rotated-refresh-token persistence, no one-time
authorize step. We therefore dropped the speculative OAuth path entirely
(`ServiceCredentials::GitLabOAuth` + `GitLabOAuthConfig` removed); `pat` is the
only GitLab flow.

### Provisioning the bot token
A Group Access Token is preferred (one identity covering every project in the
group); a Project Access Token is the per-repo equivalent.

- **Scopes:** `api` (notes + MRs + webhook registration) **and**
  `write_repository` (git push over token-HTTPS).
- **Role:** Maintainer (or Owner) — needed to register project webhooks and push
  to protected branches.
- **Expiry:** GitLab caps access-token lifetime at **365 days**.

Mint it with the GitLab CLI (or Settings → Access Tokens in the UI):

```bash
glab token create --group my-group \
  --name agent-bot \
  --scope api --scope write_repository \
  --role maintainer \
  --expires-at 2027-06-08
```

Store the printed token as the service's `token` with `auth_kind = 'pat'` (via
`POST /api/git_services` or the SPA). The `bot_username` is the generated
`group_NNN_bot_agent-bot` handle — used only for display; the loop guard is the
`BOT_NOTE_MARKER` on every posted note, not actor comparison.

### Rotation
The token expires within a year, so it must be rotated. Two options:

- **Manual:** mint a fresh token before expiry and PATCH the service `token`.
- **Automated:** the access-tokens API can rotate in place —
  `POST {base_url}/api/v4/groups/{id}/access_tokens/{token_id}/rotate` (or the
  `/projects/...` equivalent) revokes the old token and returns a new one with a
  fresh expiry; persist the new value back to the service `token`. Not wired
  today — documented for when unattended rotation is needed.

### Actor confirmation (end to end)
- **Notes / MRs:** posted via `GitLabClient` with the bot token → authored by the
  bot user. The `BOT_NOTE_MARKER` self-comment guard stays, so the bot never
  reacts to its own posts.
- **Push:** authenticates as the bot through token-HTTPS (#22, `HttpsAuth`) —
  `https://oauth2:<token>@gitlab.com/…` via the credential helper, not the
  operator's SSH key.
- **Commit authorship** is intentionally out of scope (per `.claude/CLAUDE.md`).

## Token-cache shape (GitHub only)

`resolve_token` is async and called per request precisely so the GitHub App path
can hide a cache:

```
service_id → { token, expires_at }
```

GitHub refreshes purely in memory (the installation token is re-mintable from the
PEM at will). A `RwLock<HashMap<Uuid, CachedToken>>` alongside the registry is
enough at single-operator scale. GitLab needs no cache — the bot token is a
long-lived static value resolved straight through `Pat`.
