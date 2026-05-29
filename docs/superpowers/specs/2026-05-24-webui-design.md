# claude-manager Web UI — Tauri + React

**Date:** 2026-05-24
**Status:** Approved, implementing

## Summary

Add a Tauri-based web UI alongside the existing TUI. Restructure into a Cargo workspace with a shared core crate.

## Workspace layout

```
claude-manager/
├── Cargo.toml          ← workspace root
├── crates/
│   ├── core/           ← shared: session/, store/ (lib crate)
│   ├── tui/            ← ratatui binary
│   └── webui/          ← Tauri 2.x app
│       ├── src/        ← Rust backend (commands.rs)
│       └── frontend/   ← React + TS + Vite + Tailwind
```

## Core crate API

- `discover(root) → Discovery { sessions, warnings }`
- `open_session(path, label) → Session`
- `parse_tail(path, offset) → ParseOutcome`
- `render_session(session) → RenderedSession` — pre-pairs tool calls, resolves tree, flattens for frontend consumption
- All public types derive `Serialize` for JSON transport

## Tauri commands

| Command | Core call | Return |
|---|---|---|
| `list_sessions` | `discover` | `Vec<SessionSummary>` |
| `open_session` | `open_session` + `render_session` | `RenderedSession` |
| `tail_session` | `parse_tail` | new events |

Live tail: `notify` watcher in Rust emits `session-updated` events via Tauri's event system.

## Frontend

- Sidebar: filter input, session list (project, title, time, tokens)
- Main pane: transcript with collapsible tool calls, hidden thinking toggle
- Dark theme, monospace code, proportional text
- `react-markdown` for assistant prose
- `@tauri-apps/api` for IPC + event listening
