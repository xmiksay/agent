---
name: check
description: Run the full build + test verification for the agent service before declaring a change done. Use when asked to verify, check, or confirm the agent repo is green — lint (Rust fmt + clippy, frontend typecheck) and tests, in the order rust-embed requires. Wraps `make verify`.
---

# /check — verify the agent service

Verify the repo is green before any change is called done. Always drive this through the **Makefile** — do not retype raw cargo/npm commands.

## Steps

1. **Node version.** If `nvm` is available, run `nvm use` first (version pinned in `.nvmrc`).
2. **Run the gate:**
   ```bash
   make verify
   ```
   `verify` = `lint` (`cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `npm run typecheck`) + `test` (`cargo test`).
3. **If a full build / local run is also requested**, use `make build` or `make run` — never `cargo build`/`cargo run` directly. The SPA is baked into the binary by **rust-embed**, so `frontend/dist/` must exist first; the `build`/`run` targets produce it before invoking cargo. A bare `cargo build` will embed a stale or missing bundle.

## Reporting

- Report pass/fail per stage with the actual output. Never claim green on red.
- On clippy/typecheck failures, fix them (warnings are errors here) and re-run `make verify`.
- Flag any source file pushed over the **400-line cap** — and note that `src/jobs/store.rs` and `src/jobs/runner.rs` are already over it and should be split, not grown.

## Notes

- `make check` is the fast Rust-only typecheck (no SPA) for tight inner loops; `make verify` is the pre-done gate.
- `CARGO_BUILD_JOBS` defaults to 4 in the Makefile to avoid CPU overload.
