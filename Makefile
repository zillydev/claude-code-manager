.PHONY: tui webui build app install test clean

# Vite dev-server port for `make webui` (Tauri's devUrl is overridden to match).
# Override e.g. `make webui PORT=3000`.
PORT ?= 5173

# Launch the terminal UI
tui:
	cargo run -p claude-manager-tui

# Launch the web UI (Tauri dev mode).
# --no-watch disables Tauri's Rust file watcher so editing source never
# rebuilds + relaunches the app (the relaunch stole focus and yanked the
# window onto whatever macOS Space was active). Frontend edits still
# hot-reload via Vite HMR. To pick up Rust changes, stop and re-run this.
# PORT overrides the Vite port; --config overrides Tauri's devUrl to match it.
webui:
	cd crates/webui && CM_PORT=$(PORT) cargo tauri dev --no-watch --config '{"build":{"devUrl":"http://localhost:$(PORT)"}}'

# Build everything (Rust workspace + frontend) — a compile/CI check, no bundle.
build:
	npm --prefix crates/webui/frontend install
	npm --prefix crates/webui/frontend run build
	cargo build --release

APP_NAME := claude-manager.app
APP_BUNDLE := target/release/bundle/macos/$(APP_NAME)

# Build the distributable macOS app: `cargo tauri build` runs the frontend
# production build (beforeBuildCommand), then compiles + bundles the release
# binary into target/release/bundle/ (.app + .dmg). npm install first because
# the before-command only runs `npm run build`, not install.
app:
	npm --prefix crates/webui/frontend install
	cd crates/webui && cargo tauri build

# Build the app and install it into /Applications so the Dock icon updates.
# ditto replaces the existing copy; a locally-built app carries no quarantine
# attribute, so Gatekeeper won't prompt on launch.
install: app
	rm -rf "/Applications/$(APP_NAME)"
	ditto "$(APP_BUNDLE)" "/Applications/$(APP_NAME)"
	@echo "Installed $(APP_NAME) -> /Applications (quit + reopen if it was running)"

# Run all tests
test:
	cargo test

# Print session list (quick smoke test)
print:
	cargo run -p claude-manager-tui -- --print

clean:
	cargo clean
	rm -rf crates/webui/frontend/dist crates/webui/frontend/node_modules
