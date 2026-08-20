# claude-code-manager — Session status publishing

**Date:** 2026-08-21 · **Status:** Approved
**Goal:** Let ccm publish live session state (running / unread / idle) to a documented JSON file that any third-party tool can read. First consumer: a sketchybar space indicator, replacing a hook-driven state file that had gone permanently stale.

## Why not hooks

The existing indicator was fed by Claude Code hooks writing `~/.config/sketchybar/claude_sessions.json`: `UserPromptSubmit` → `working`, `Stop` → `idle`, `Notification` → `attention`. Measured on the user's machine: **406 `UserPromptSubmit` fires vs 14 `Stop`**, the last `Stop` a day earlier despite hundreds of prompts. Every one of the 18 entries was stuck at `working`, some 13 hours old, so the indicator (which only lights on `idle`/`attention`) never lit.

ccm derives the same facts without hooks. Its app-wide poller runs `store::scan_activity` every 3s: one `stat` per session file, a 64 KB tail-read of anything touched in the last 60s, and `Event::awaits_user()` for turn-completeness. That is self-correcting — a missed event can't strand a session, because the next tick re-reads the truth.

## What ccm knows that hooks don't (and vice versa)

The hook wrapper (`claude-tracked`) captures the *focused yabai window* at launch and freezes it in the environment; the hook re-resolves that window's space per event. ccm reads `.jsonl` files and has no notion of macOS windows — **except** for sessions it hosts in a terminal tab, where `TerminalState.owner(session)` gives the owning Tauri window label. That window's `NSWindow.windowNumber` is the same integer yabai and sketchybar use (verified: yabai ids `61729, 61918, 61893, 59457, 49487, 49275` all appear in `CGWindowListCopyWindowInfo` for the app's pid).

All of the user's live tracked sessions run in ccm terminal tabs, so ccm can attribute every one of them — more accurately than the wrapper, since it knows when a terminal or window closes rather than holding a launch-time snapshot.

## The file

Path resolves exactly like the rest of ccm's files — `$CLAUDE_CODE_MANAGER_CONFIG_DIR` → `$XDG_CONFIG_HOME/claude-code-manager` → `~/.config/claude-code-manager` — and is named **`session-status.json`**, beside `session-meta.json`. The naming carries the split: *meta* is durable user data, *status* is live derived state, disposable and rebuilt from the `.jsonl` files.

```json
{
  "version": 1,
  "updated": 1787263639.1,
  "sessions": {
    "59a0dc6a-b2a0-46a7-8533-27305b978f32": {
      "state": "running",
      "awaiting_input": false,
      "last_event_at": 1787263620,
      "cwd": "/Users/zillydev/my/Dev/Personal/claude-manager",
      "title": "sync status with sketchybar",
      "window": 61729,
      "hosted": true
    }
  }
}
```

- **`state`** — `running` | `unread` | `idle`, ccm's own opinion (same derivation as the sidebar dots).
- **`awaiting_input`, `last_event_at`** — the raw facts, so a consumer that disagrees with ccm's rules can derive its own.
- **`window`** — macOS window id (= yabai id) when ccm hosts the session in a terminal tab; `null` otherwise. A non-hosted session is still reported, it just can't be placed on a space.
- **`hosted`** — whether ccm is running this session's `claude` process.
- **`title`, `cwd`** — for display by consumers.

**Which sessions appear:** running, unread, or hosted. Not the whole corpus — a file of every session ever run is noise, and sessions never opened are never unread (the `lastViewed` baseline), so this answers "what is alive or wants me".

**Guarantees:** atomic replace (temp file + rename), rewritten only when the content changes, and deleted when publishing is switched off or the app exits — a status file that outlives the app is worse than no file.

## State derivation

Reuses the sidebar's semantics (`lib/sessionStatus.ts`), lifted into Rust:

| state | condition |
|---|---|
| `running` | `!awaiting_input && now - last_event_at < 180s` (staleness backstop; a completed turn ends "running" at once) |
| `unread` | `awaiting_input && last_event_at > last_viewed` |
| `idle` | `awaiting_input && last_event_at <= last_viewed` (finished and already seen) |

`awaits_user()` covers both a completed `end_turn` and a turn parked on a user-blocking tool (`AskUserQuestion` / `ExitPlanMode`).

`lastViewed` currently lives in frontend `localStorage` (`cm:lastViewed`, synced across windows via the `storage` event). Rather than refactor it, each window pushes its map to the backend on change via a new `set_last_viewed` command; `Shared` keeps the per-session **maximum**, so windows can't fight. Without it there is no `idle`: every finished session would read `unread` forever and re-badge every time you passed its space.

## Opt-in

New config block, default **off** — writing files is a side effect the user chooses:

```rust
#[serde(default)] pub integrations: Integrations,
pub struct Integrations { #[serde(default)] pub publish_status: bool }
```

Settings gains an **Integrations** section with one `CheckRow` — "Publish session status" — plus the resolved path in dim text beneath it. Off → on: the next tick (≤3s) writes the file. On → off: the file is deleted. Config already broadcasts `config-changed`, so the poller simply reads the current value each tick.

## Consumer: sketchybar

`~/.config/sketchybar/plugins/space.sh` (outside this repo; original kept as `.bak`) drops the hook-file block entirely and reads `session-status.json`: build an `id → space` map from one `yabai -m query --windows`, keep `.sessions` entries whose `window` maps to this space, then badge 󰚩 orange when any is `unread` (excluding the focused window) or green when any is `running`. Space is resolved live rather than stored, so a window moved between spaces badges correctly and a closed window's entry drops out on its own.

No `sketchybar --trigger`: the space items already run `space.sh` every 5s (`update_freq=5`), and one trigger would re-run the script for all 21 space items, each shelling out to yabai and jq.

## Files

- New: `crates/app/src/status_file.rs` — the pure `desired_state(...) → StatusFile` mapping plus the atomic writer/remover.
- Touched: `crates/app/src/config.rs` (`Integrations`), `shared.rs` (merged `last_viewed` map), `lib.rs` (poller writes after each tick; clear on exit), `commands.rs` (`set_last_viewed`), `terminal.rs` (expose `hosted()` — session id → owner label), `window.rs` or a small helper (label → `NSWindow.windowNumber`), frontend `Settings.tsx` + `useConfig`/`config.ts` types, and a `useEffect` pushing `lastViewed`.
- Docs: README section describing the file as a public interface; FEATURES.md entry (new capability).

## Testing

Pure Rust tests for the mapping: running / unread / idle per the table, the 180s backstop, unhosted sessions carrying `window: null`, sessions that are neither running/unread/hosted excluded, and the change-detection that suppresses redundant writes. The window-id lookup and the sketchybar script are verified by the user in situ (merge-gated).
