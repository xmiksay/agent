# agent service — build/test/dev targets.
# Node version is pinned in .nvmrc (run `nvm use` first if needed).
# IMPORTANT: the SPA is baked into the binary by rust-embed, so frontend/dist/
# must exist before any cargo build/run — `build`, `run`, and `verify` enforce that.

export CARGO_BUILD_JOBS ?= 4

FRONTEND := frontend

.PHONY: all frontend build check fmt lint test verify dev run clean

all: build

## Build the SPA bundle into frontend/dist/ (prereq for any cargo build)
frontend:
	cd $(FRONTEND) && npm ci && npm run build

## Full release build: SPA first, then the binary that embeds it
build: frontend
	cargo build --release

## Fast Rust typecheck (no SPA needed)
check:
	cargo check

## Apply Rust formatting
fmt:
	cargo fmt

## Lint everything: Rust format-check + clippy, frontend typecheck
lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cd $(FRONTEND) && npm run typecheck

## Run the test suite
test:
	cargo test

## The pre-"done" gate: lint + tests must be green before declaring a change done
verify: lint test

## Hot-reload frontend (vite on 5173, proxies API to the agent)
dev:
	cd $(FRONTEND) && npm run dev

## Run the service locally (SPA baked in, migrations run on startup)
run: frontend
	cargo run

clean:
	cargo clean
	rm -rf $(FRONTEND)/dist
