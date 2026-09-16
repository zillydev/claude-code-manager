# Session links — design

## Problem

There is no way to point at a session from outside the app. Notes about work (an Obsidian vault, a Markdown file) can name a session but not open it; getting back to it means finding it in the sidebar by title or id.

A session link should be something you copy from the app, paste into a note, and click later: the app opens — launching it if it isn't running — and shows that session.

## Decisions

- **Link format.** `claude-code-manager://session/<session-uuid>`. The scheme matches the bundle name, so it can't collide with another app's; the `session/` path leaves room for other link kinds (a message) without changing these. Anything else — another scheme, another path, an invalid uuid — is ignored. A trailing slash is tolerated.
- **Where links are used.** Obsidian and Markdown notes, which open custom schemes. No http redirect: Slack and other apps that only link http(s) are out of scope.
- **Copies a bare link.** "Copy link" puts the bare URL on the clipboard, not a Markdown link: you write the link text in the note.
- **`tauri-plugin-deep-link`.** Registered in `lib.rs`; `tauri.conf.json` declares `plugins.deep-link.desktop.schemes: ["claude-code-manager"]` and the plugin writes `CFBundleURLTypes` into the bundle's `Info.plist`. macOS can't register a scheme at runtime and needs no single-instance plugin (links arrive through `application:openURLs:`, not argv). No capability permission: links are handled in Rust and the frontend never calls the plugin's JS API.
- **Installed app only.** macOS routes the scheme to the bundle in `/Applications`, so links reach the app built by `make install`, never `make dev`. A link clicked while working in the dev app opens the installed app.
- **Receiving.** In `setup`, `deep_link().on_open_url` passes each URL to one `handle_link`; `get_current()` is passed through it once at startup for a link that launched the app. On macOS a launch link can also arrive as an open event, so the same session id within 2 s of the last handled link is ignored.
- **Choosing the window.** A pure function over plain data picks, in order: a window with that session open; the window running its terminal (`TerminalState` owner); the last focused window; any window; a new window when none exist.
- **Link state.** `LinkState` in the backend holds each window's open session (set by `open_session`, cleared by `stop_watching` and on `Destroyed`), the last focused window (`WindowEvent::Focused(true)`), the windows whose frontend is listening, and one pending link.
- **Delivering.** The chosen window is unminimised and focused — on macOS that switches to its Space — and receives `open-session-link` with the session id.
- **Before a window is ready.** A link that launches the app, or opens a new window, arrives before any frontend listens, and an event emitted then is dropped. The link is stored as pending; each window's frontend attaches its listener and then calls `link_listener_ready`, which re-runs routing and delivers the pending link to the window it picks. Delivery only ever goes to a listening window: when the picked window isn't listening yet, the link stays pending until it is. A newer pending link replaces an older one.
- **Copying.** "Copy link" sits after "Note…" in a session row's ⋯ menu and right-click menu. The link is built by `lib/sessionLink.ts`. Menus close on click, so the row confirms instead: its relative time reads "link copied ✓" for 1.2 s, the label swap the "copy id" and "copy resume" buttons use.
- **Opening.** A received id is held until the session list has loaded (a launch link beats discovery). Found → `openSession`, the same path as clicking the row: Manager tab unless its terminal is live, sidebar highlight follows. Not found → one `refresh()` (a new file not yet discovered), then a dismissible notice at the top of the session pane: "No session with that id — it may have been deleted, or lives on another machine." Sidebar filters are left alone: a filtered-out session still opens in the pane.

## Rejected

- **Hand-written `Info.plist` + `RunEvent::Opened`, no plugin.** Both pieces exist without the plugin on macOS (the bundler merges a local `Info.plist`; tao forwards `application:openURLs:`), but the official plugin was preferred.
- **Broadcasting links to every window, each deciding.** No new backend state, but windows race: two can claim a link, or none.
- **Markdown link on copy; http redirect for Slack.** Links are pasted into notes that take a bare custom-scheme URL.
- **Always a new window / always the last used window.** A new window per link piles windows up; ignoring where the session already is competes with its terminal.

## Testing

- Unit (Rust): link parsing — valid, wrong scheme, non-`session/` path, invalid uuid, trailing slash; window choice — each priority step in order, destroyed windows ignored; dedupe — same link within 2 s ignored, a different one not.
- Unit (TS): `sessionLink` format; pending-link resolution — found, found after refresh, not found.
- Gate: `cargo clippy --workspace --all-targets -- -D warnings`, both test suites, both builds.
- Manual, installed app (`make install`): a link while running; to a session a window already shows; to a session whose terminal is in another window; with the app quit (launches and opens); an unknown id; clicked from an Obsidian note.
