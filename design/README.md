# Agent redesign — "Instrument" design system

A new visual language for the live **Agent** frontend (`agent/frontend`, the
`claude-agent-ui` SPA). Built on **Tailwind** (kept, per the redesign decision)
and delivered **design-system-first**: foundations + components, then screens.

This folder *is* the redesign: a real Vue/Vite/Tailwind app on **dummy data**,
mirroring the live stack so the result ports back to `agent/frontend` cleanly.

## Direction

An operator's control surface for a fleet of autonomous coding agents. Calm
graphite cockpit, hairline structure, monospaced data, one decisive **amber**
signal accent, and status rendered as **glowing LEDs** (cyan = running,
green = done, red = failed, violet = awaiting-auth, orange = releasing).
Dense and legible like a terminal — deliberately not the neon-hacker cliché.

- **Type:** Bricolage Grotesque (display) · Archivo (UI body) · IBM Plex Mono (data)
- **Accent:** `#ffb02e` on a cool near-black canvas `#0c0e12`

## Run it

```sh
cd agent/design && npm install && npm run dev   # http://127.0.0.1:5180/
```

The shell ships **two chrome layouts** — toggle **Top ↔ Side** with the switch
in the bottom-right (or share a link with `?layout=top` / `?layout=side`).

## Layout

| Path | Purpose |
|---|---|
| `tailwind.config.ts` | The "Instrument" theme — the portable artifact (drop into `agent/frontend`). |
| `src/style.css` | `@layer components` — the `.btn` / `.input` / `.pill` / `.led` / `.tbl` system. |
| `src/components/` | Redesigned SFCs (StatusPill, ProviderBadge, TriggerView, MarkdownView, Accordion, DiffView, ClaudeStream, AuthApprovalForm, NewTaskModal, Logo). |
| `src/views/` | Screens: Tasks, TaskDetail, Projects, GitServices, AuthRequestsQueue, Stats. |
| `src/App.vue` | App shell with the Top/Side layout switch + router. |
| `src/fixtures.ts` | Mock data layer — stands in for the live Pinia stores / API. |
| `templates/` | The 21 templates extracted from the live frontend (the redesign targets). |

## Porting to the live app (`agent/frontend`)

The app mirrors the live stack, so the port is mostly a file copy:

1. **Theme** — copy the `extend` block from `tailwind.config.ts` into
   `agent/frontend/tailwind.config.ts` (replacing the current `ink` stub).
2. **System classes** — move the `@layer components` block from `src/style.css`
   into `agent/frontend/src/style.css` (it already hosts `.tbl`).
3. **Fonts** — add the Google Fonts `<link>` from `index.html`.
4. **Components** — copy each `src/components/*.vue` over its live counterpart
   (props/logic are identical; only the template/styles changed).
5. **Views** — port each view's `<template>`; the live `<script>` (stores,
   router, composables) stays.

## Status

- ✅ Direction + foundations (color, type, radius), `.btn`/`.input`/`.tbl` system
- ✅ Components: StatusPill, ProviderBadge, TriggerView, MarkdownView, Accordion,
  DiffView, ClaudeStream, AuthApprovalForm, NewTaskModal, the Agent **Logo**
- ✅ App shell with **Top/Side layout switch** + router; views: Tasks, TaskDetail,
  Projects, Git services, Auth queue, Stats — on a mock data layer (no backend)
- ⏳ Remaining views: ProjectDetail, GitServiceDetail, AuthRequestDetail;
  components: InlineAuthApproval, TokenGate
- ⏳ Port the system into `agent/frontend`
