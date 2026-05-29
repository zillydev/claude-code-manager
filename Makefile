.PHONY: tui webui build test clean

# Launch the terminal UI
tui:
	cargo run -p claude-manager-tui

# Launch the web UI (Tauri dev mode)
webui:
	cd crates/webui && cargo tauri dev

# Build everything (Rust workspace + frontend)
build:
	npm --prefix crates/webui/frontend install
	npm --prefix crates/webui/frontend run build
	cargo build --release

# Run all tests
test:
	cargo test

# Print session list (quick smoke test)
print:
	cargo run -p claude-manager-tui -- --print

clean:
	cargo clean
	rm -rf crates/webui/frontend/dist crates/webui/frontend/node_modules
