# Convenience targets. The real build is plain `cargo`.
#
# The workspace has three crates:
#   apfsrelic-core   dependency-free engine (the auditable part)
#   apfsrelic-cli    the `apfsrelic` binary
#   apfsrelic-gui    Tauri v2 desktop GUI (heavy deps; needs a webview toolkit)
#
# The default `ci` gate runs against core + cli only, keeping it fast and
# free of system dependencies. Use the `gui-*` targets for the GUI.

CORE_CLI := -p apfsrelic-core -p apfsrelic-cli

.PHONY: all build release test fmt clippy ci gui gui-dev gui-build gui-check \
        frontend-install frontend-build clean fuzz

all: build

# Build just the CLI + engine (no GUI toolchain required).
build:
	cargo build $(CORE_CLI)

release:
	cargo build --release $(CORE_CLI)

test:
	cargo test $(CORE_CLI)

fmt:
	cargo fmt --all

clippy:
	cargo clippy $(CORE_CLI) --all-targets -- -D warnings

# The same gate CI enforces for the auditable core + cli.
ci:
	cargo fmt --all -- --check
	cargo clippy $(CORE_CLI) --all-targets -- -D warnings
	cargo test $(CORE_CLI)

# --- GUI (Tauri v2 + React/Carbon frontend) ---
# The webview lives in ./frontend (Vite + React + @carbon/react). Tauri picks
# what to load at COMPILE time via `tauri::is_dev()` == `!cfg(custom-protocol)`:
#   * with    `custom-protocol` -> serve the embedded `frontendDist` (static).
#   * without `custom-protocol` -> load `devUrl` (the Vite dev server) — a blank
#     window unless that server is running.
# So `make gui` builds the bundle and runs with the feature (self-contained),
# and `make gui-dev` is the hot-reload loop (Vite + a dev-mode GUI).
FRONTEND := frontend

# Install frontend deps (run once, or after package.json changes).
frontend-install:
	cd $(FRONTEND) && pnpm install

# Build the webview bundle into frontend/dist.
frontend-build:
	cd $(FRONTEND) && pnpm build

# Run the desktop app statically: build the webview, then launch the GUI with
# the embedded assets (no dev server needed).
gui: frontend-build
	cargo run -p apfsrelic-gui --features custom-protocol

# Hot-reload dev loop: start Vite (the devUrl target) in the background, then run
# the GUI in dev mode so it loads http://localhost:1420 with live reload. Vite is
# stopped when you quit the app.
gui-dev:
	cd $(FRONTEND) && pnpm dev & \
	  VITE_PID=$$!; \
	  trap "kill $$VITE_PID 2>/dev/null" EXIT; \
	  sleep 2; \
	  cargo run -p apfsrelic-gui

gui-build: frontend-build
	cargo build --release -p apfsrelic-gui --features custom-protocol

gui-check:
	cargo clippy -p apfsrelic-gui --all-targets -- -D warnings

fuzz:
	cd fuzz && cargo +nightly fuzz run parse_btree

clean:
	cargo clean
