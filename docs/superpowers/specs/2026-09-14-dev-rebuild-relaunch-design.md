# Dev Rebuild & Relaunch — design

## Problem

`make dev` ran `cargo tauri dev --no-watch`. With the watcher on, every Rust edit (usually Claude's) killed and relaunched the app, and the new window stole focus and opened on whatever macOS Space was active. With it off, picking up a Rust change meant quitting the app and re-running `make dev` by hand.

The dev app is a test bench: its embedded terminals and process state are disposable. What matters is *when* the relaunch happens. A relaunch while you are looking at the dev app opens on that Space and taking focus is harmless — so relaunch only on an explicit request from inside the app.

## Decisions

- **On demand only.** No file watching; Develop → Rebuild & Relaunch (⌘⇧R, a native menu item so it works with the terminal focused). Background builds were rejected for heat/CPU.
- **Own launcher, not `tauri dev`.** tauri-cli 2.11 SIGKILLs the app *before* compiling, leaves no app after a compile error, and exits the whole session on any app exit. `scripts/dev.sh` instead runs Vite once, builds, and runs the binary in a loop: exit code `75` → run it again, anything else → stop Vite and exit.
- **Build first, swap on success.** The app runs `cargo build` itself while its windows stay up (a `Rebuilding…` banner), tees stderr to the `make dev` terminal, and on failure shows the error excerpt with Retry — the old build keeps running. On success it writes the restore file and exits with `75`.
- **Equivalent builds.** `generate_context!` bakes `devUrl` in at compile time from `TAURI_CONFIG`; the launcher exports `{"build":{"devUrl":"http://localhost:$PORT"}}` and the app's rebuild inherits the identical string, so the build script doesn't rerun and builds stay incremental.
- **Gate.** `debug_assertions` AND `CM_DEV_LAUNCHER=1` (set by the launcher). A debug binary started any other way has nothing to relaunch it, so the menu, commands and restore are off.
- **Exit code.** Tauri's `AppHandle::exit(code)` loses the code on macOS (tao turns the request into a plain `ControlFlow::Exit`), so the `RunEvent::Exit` handler, after the app's own cleanup, calls `cleanup_before_exit()` and `std::process::exit(75)`.
- **Restore.** Each window reports `{ target, sidebarTab }` via `dev_set_view` as it changes (held in memory, not localStorage, which may not flush before an immediate exit). The rebuild writes `dev-restore.json` (config dir) with every window's label, logical frame and view. At startup the backend consumes the file (ignored after 120 s), repositions `main`, recreates `win-N` windows, and hands each view out once via `dev_take_view`. Restored windows open on Manager and skip the launch default tab: landing on Terminal would spawn `claude --resume` unasked.

## Rejected

- **`tauri dev` watcher triggered by a file** (`.taurignore` `/?*` + `!/reload/`, plus one in `crates/core`): less code, but the window vanishes for the whole compile and a compile error leaves no app to retry from.
- **Rust hot-patching (`subsecond`)**: needs Dioxus's `dx` tooling, unproven with Tauri.

## Testing

- Unit: error excerpt (first error onward, capped, tail fallback); restore file round-trip, consumed on read, stale and malformed ignored.
- Launcher, with a stub binary: exits `75, 75, 0` relaunch twice then stop Vite; the child gets `crates/app` cwd, `CM_DEV_LAUNCHER`, `CM_PORT`, `TAURI_CONFIG`; SIGTERM to the script kills Vite and the app.
- Manual (GUI): ⌘⇧R success relaunch with multiple windows and open sessions restored; a deliberate compile error shows the banner and keeps the app; normal quit ends `make dev`.
