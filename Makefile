# agent service — build/test/dev targets.
# Node version is pinned in .nvmrc; npm recipes source nvm and `nvm use` it
# automatically (falls back to whatever node is on PATH if nvm isn't installed).
# IMPORTANT: the SPA is baked into the binary by rust-embed, so frontend/dist/
# must exist before any cargo build/run — `build`, `run`, and `verify` enforce that.

export CARGO_BUILD_JOBS ?= 4

FRONTEND := frontend

# Load nvm (if present) and select the Node version pinned in .nvmrc. Prefix this
# to any npm invocation; `nvm use` reads the .nvmrc of the current directory.
NVM_DIR ?= $(HOME)/.nvm
USE_NODE = if [ -s "$(NVM_DIR)/nvm.sh" ]; then . "$(NVM_DIR)/nvm.sh" && nvm use; fi

.PHONY: all frontend build check fmt lint test verify dev run clean

all: build

## Build the SPA bundle into frontend/dist/ (prereq for any cargo build)
frontend:
	cd $(FRONTEND) && $(USE_NODE) && npm ci && npm run build

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
	cd $(FRONTEND) && $(USE_NODE) && npm run typecheck

## Run the test suite
test:
	cargo test

## The pre-"done" gate: lint + tests must be green before declaring a change done
verify: lint test

## Hot-reload frontend (vite on 5173, proxies API to the agent)
dev:
	cd $(FRONTEND) && $(USE_NODE) && npm run dev

## Run the service locally (SPA baked in, migrations run on startup)
run: frontend
	cargo run

## Build the release binary and restart the systemd service
deploy: frontend
	touch src/lib.rs
	cargo build --release
	sudo systemctl stop agent
	sudo cp target/release/agent /usr/local/bin/agent
	sudo systemctl start agent

clean:
	cargo clean
	rm -rf $(FRONTEND)/dist
