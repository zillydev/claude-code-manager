.PHONY: dev build app install test print clean

# Vite dev-server port for `make dev` (Tauri's devUrl is overridden to match).
# Override e.g. `make dev PORT=3000`.
PORT ?= 5173

# Launch the desktop app in dev mode (scripts/dev.sh). Frontend edits hot-reload
# via Vite HMR. Rust edits never restart the app on their own (a restart steals
# focus and yanks the window onto the active macOS Space): pick them up with
# Develop → Rebuild & Relaunch (⌘⇧R), which builds while the window stays up and
# relaunches only on success, restoring windows and open sessions.
dev:
	PORT=$(PORT) scripts/dev.sh

# Build everything (Rust workspace + frontend) — a compile/CI check, no bundle.
build:
	npm --prefix crates/app/frontend install
	npm --prefix crates/app/frontend run build
	cargo build --release

APP_NAME := Claude Code Manager.app
APP_BUNDLE := target/release/bundle/macos/$(APP_NAME)

# Build the distributable macOS app: `cargo tauri build` runs the frontend
# production build (beforeBuildCommand), then compiles + bundles the release
# binary into target/release/bundle/ (.app + .dmg). npm install first because
# the before-command only runs `npm run build`, not install.
app:
	npm --prefix crates/app/frontend install
	cd crates/app && cargo tauri build

# Build the app and install it into /Applications so the Dock icon updates.
# ditto replaces the existing copy; a locally-built app carries no quarantine
# attribute, so Gatekeeper won't prompt on launch.
install: app
	rm -rf "/Applications/$(APP_NAME)" "/Applications/claude-code-manager.app"
	ditto "$(APP_BUNDLE)" "/Applications/$(APP_NAME)"
	@echo "Installed $(APP_NAME) -> /Applications (quit + reopen if it was running)"

# Run all tests
test:
	cargo test

# Print session list (quick smoke test of the CLI + core)
print:
	cargo run -p claude-code-manager-cli -- --print

clean:
	cargo clean
	rm -rf crates/app/frontend/dist crates/app/frontend/node_modules
