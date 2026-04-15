# GitLab Claude Agent — Implementační plán

## Přehled projektu

Rust HTTP server naslouchající GitLab webhookům, který spouští Claude Code
jako autonomního agenta pro zpracování issues a code review.

```
GitLab ──webhook──► Rust Server ──spawn──► claude -p "..."
                         │                      │
                    job queue              glab, git
                         │                      │
                    GitLab API ◄── komentáře, MR, push
```

---

## Fáze 1 — Základ serveru

**Cíl:** Funkční HTTP server přijímající a ověřující GitLab webhooky.

### Struktura projektu

```
gitlab-claude-agent/
├── Cargo.toml
├── .env.example
├── README.md
├── src/
│   ├── main.rs
│   ├── config.rs          # konfigurace z env proměnných
│   ├── webhook/
│   │   ├── mod.rs
│   │   ├── handler.rs     # axum route handler
│   │   ├── types.rs       # GitLab payload structs
│   │   └── verify.rs      # ověření X-Gitlab-Token
│   ├── jobs/
│   │   ├── mod.rs
│   │   ├── queue.rs       # job fronta + semaphore
│   │   ├── runner.rs      # spouštění claude procesu
│   │   └── types.rs       # ClaudeJob, JobStatus, TriggerReason
│   └── gitlab/
│       ├── mod.rs
│       └── client.rs      # volání GitLab API (komentáře, MR info)
├── claude/
│   └── CLAUDE.md          # instrukce pro agenta (šablona)
└── systemd/
    └── gitlab-claude-agent.service
```

### Cargo.toml závislosti

```toml
[dependencies]
axum = "0.7"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tower = "0.4"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
uuid = { version = "1", features = ["v4"] }
reqwest = { version = "0.12", features = ["json"] }
dotenvy = "0.15"
thiserror = "1"
anyhow = "1"
```

### Checklist Fáze 1

- [ ] `cargo new gitlab-claude-agent`
- [ ] Základní axum server na `POST /webhook/gitlab`
- [ ] Deserializace GitLab payloadu (MergeRequest, Note, Issue)
- [ ] Ověření `X-Gitlab-Token` hlavičky
- [ ] Konfigurace přes env proměnné (`WEBHOOK_SECRET`, `GITLAB_TOKEN`, `ANTHROPIC_API_KEY`)
- [ ] Zdravotní endpoint `GET /health`
- [ ] Logování přes `tracing`

---

## Fáze 2 — Job runner

**Cíl:** Spolehlivé spouštění `claude -p` procesu a zpracování výstupu.

### Klíčové vlastnosti

- Tokio `Semaphore` pro omezení paralelních jobů (default: 3)
- `tokio::time::timeout` — každý job max 10 minut
- Idempotence přes `HashSet<String>` uložených event ID
- Parsování `--output-format json` výstupu

### ClaudeOutput struct

```rust
#[derive(Deserialize)]
pub struct ClaudeOutput {
    pub result: String,
    pub session_id: String,
    pub total_cost_usd: f64,
    pub is_error: bool,
    pub num_turns: u32,
}
```

### Checklist Fáze 2

- [ ] `ClaudeJob` struct (prompt, allowed_tools, max_turns, budget, callback)
- [ ] Job queue s `Arc<Semaphore>` pro max konkurenci
- [ ] `run_claude_job()` — clone repo, spusť claude, zpracuj výstup
- [ ] Parsování JSON výstupu z claude
- [ ] Deduplikace eventů (GitLab může posílat duplicity)
- [ ] Timeout handling
- [ ] Cleanup `/tmp/claude-jobs/<job_id>` po dokončení
- [ ] Logování nákladů (`total_cost_usd`) per job

---

## Fáze 3 — GitLab integrace

**Cíl:** Správná detekce triggerů a reportování výsledků zpět do GitLabu.

### Triggery

| Event | Podmínka | Akce |
|-------|----------|------|
| `issue` + `action: update/open` | `assignees` obsahuje můj username | ISSUE WORKFLOW |
| `merge_request` + `action: update/open` | `reviewers` obsahuje můj username | REVIEW WORKFLOW |
| `merge_request` + `action: unapproval` | jsem autor MR | FIX REVIEW WORKFLOW |
| `note` na MR | `body` obsahuje `@claude` | COMMENT WORKFLOW |

### Checklist Fáze 3

- [ ] `should_handle()` — routing logika pro všechny event typy
- [ ] `GitLabClient` — post komentář na MR/issue
- [ ] `GitLabClient` — získat MR detail (pro FIX REVIEW)
- [ ] Konfigurace `MY_GITLAB_USERNAME` z env
- [ ] Sestavení promptu podle trigger typu
- [ ] Push výsledků zpět (git push přes deploy key)

---

## Fáze 4 — CLAUDE.md pro agenta

**Cíl:** Agent zná projekt a umí pracovat samostatně bez dlouhých promptů v kódu.

### Struktura v repozitáři

```
(každý spravovaný repozitář)/
├── CLAUDE.md                    # hlavní kontext projektu
└── .claude/
    ├── settings.json            # permissions
    ├── rules/
    │   ├── gitlab-workflow.md   # jak pracovat s glab
    │   ├── conventions.md       # code style projektu
    │   └── testing.md           # jak spouštět testy
    └── skills/
        ├── implement-issue/
        │   └── SKILL.md         # auto-trigger pro issue workflow
        └── review-mr/
            └── SKILL.md         # auto-trigger pro review workflow
```

### Checklist Fáze 4

- [ ] Šablona `CLAUDE.md` s issue + review workflow
- [ ] `SKILL.md` pro issue implementaci (trigger: "implementuj issue")
- [ ] `SKILL.md` pro MR review (trigger: "zreviewuj MR")
- [ ] `.claude/settings.json` s `allowedTools` bílou listinou
- [ ] Dokumentace jak přizpůsobit pro konkrétní projekt

---

## Fáze 5 — Deployment

**Cíl:** Produkční nasazení jako systemd služba.

### Prerekvizity na serveru

```bash
# Závislosti
apt install git curl jq

# glab CLI
curl -sL https://github.com/cli/cli/releases/... | tar xz
mv glab /usr/local/bin/

# Claude Code
npm install -g @anthropic-ai/claude-code

# Autentizace (jednorázově)
claude setup-token   # vygeneruje CLAUDE_CODE_OAUTH_TOKEN platný 1 rok
# NEBO pro API klíč (firma):
# export ANTHROPIC_API_KEY="sk-ant-api03-..."

# SSH deploy key pro git push
ssh-keygen -t ed25519 -f ~/.ssh/gitlab_agent_deploy
# přidat public key do GitLab projektu jako Deploy Key (write access)
```

### Env proměnné

```bash
# /etc/gitlab-claude-agent/env
WEBHOOK_SECRET=<náhodný tajný token>
GITLAB_TOKEN=<gitlab personal/project access token>
CLAUDE_CODE_OAUTH_TOKEN=<sk-ant-oat01-...>   # MAX subscription
# NEBO
ANTHROPIC_API_KEY=<sk-ant-api03-...>          # Console API key (firma)
MY_GITLAB_USERNAME=<tvůj username>
REPO_BASE_PATH=/tmp/claude-jobs
MAX_CONCURRENT_JOBS=3
```

### systemd jednotka

```ini
[Unit]
Description=GitLab Claude Agent
After=network.target

[Service]
Type=simple
User=claude-agent
EnvironmentFile=/etc/gitlab-claude-agent/env
ExecStart=/usr/local/bin/gitlab-claude-agent
Restart=on-failure
RestartSec=5s
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

### Checklist Fáze 5

- [ ] Build release binary: `cargo build --release`
- [ ] Systemd unit soubor
- [ ] Konfigurace GitLab webhooků (URL, secret, eventy)
- [ ] SSH deploy key s write přístupem
- [ ] Otestovat end-to-end: vytvořit testovací issue → sledovat logy
- [ ] Monitoring: logrotate, alerting při chybách
- [ ] Dokumentace: jak přidat nový repozitář pod správu agenta

---

## Licenční model

| Situace | Auth | Plán |
|---------|------|------|
| Jen ty, lokální stroj | `CLAUDE_CODE_OAUTH_TOKEN` | MAX subscription |
| Jen ty, remote server | `CLAUDE_CODE_OAUTH_TOKEN` | MAX subscription |
| Celý tým, remote server | `ANTHROPIC_API_KEY` | Console pay-per-token |

---

## Pořadí implementace

```
Fáze 1 (server + webhooky)     ~2 dny
Fáze 2 (job runner)            ~2 dny
Fáze 3 (GitLab integrace)      ~2 dny
Fáze 4 (CLAUDE.md + skills)    ~1 den
Fáze 5 (deployment)            ~1 den
─────────────────────────────────────
Celkem                         ~8 dní
```

Doporučené pořadí: implementuj fáze 1–3 end-to-end s jednoduchým promptem
(bez skills), ověř že celý webhook → claude → GitLab komentář funguje,
pak teprve ladí CLAUDE.md a skills (Fáze 4).
