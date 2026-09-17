# claude-code-manager — Roadmap

Larger changes worth doing that need a design before they are worth starting. Day-to-day items live in `TODOS.md`; what already exists is in `FEATURES.md`.

> **Maintenance:** an entry leaves this file when its work merges — at that point `FEATURES.md` (new capability) or `CHANGELOG.md` (everything else) carries the story, and the design spec in `docs/superpowers/specs/` carries the rationale.

## Virtualize the transcript

A window renders every message of the open session into the document at once, so its memory tracks the size of the session rather than what is on screen. Rendering only what is near the viewport, with measured placeholders for the rest, would keep a window roughly flat whatever the session's size.

**Measured (2026-09-17, one window, installed build).** WebKit's own baseline is ~60–90 MB before the app's own content. On top of that, a 1.3 MB session costs ~90 MB and the largest session here (67.6 MB) drives the window process to ~900 MB. WebKit gives that memory back slowly: minutes after switching to a small session the window still held 747 MB, so the cost outlives the session that caused it. The window shows up in Activity Monitor as `tauri://localhost` — WebKit names a render process after the page's origin — and there is one per window.

**What it has to keep working.** Scroll position memory, jump-to-message from global find and from a bookmark, find-in-page inside a session, the live tail appending turns, and message heights that vary wildly (a one-line prompt next to a 2000-line tool result). Each of those currently relies on every message existing in the document, which is what makes this a design rather than a patch.

## Custom ordering of sessions in the sidebar

The sessions list sorts by the chosen sort (updated, created, title) and nothing else, so a session you keep coming back to sinks as others get touched. Worth discussing before building, because the approaches differ in how much state they add and how they interact with the existing filters and sort:

- **Pinned rows.** A pin per session, pinned ones held in their own group at the top, everything below in the normal sort. Smallest change, one flag per session, and it survives any sort or filter.
- **Manual drag order.** Drag rows into whatever order you like. The most control, but it needs a position per session, an answer for where a new session lands, and it conflicts with sorting — the list is either yours or sorted, not both.
- **Named sections.** Group sessions under headings you define, sorted within each. More structure than pinning, and it overlaps with tags, which already group sessions.
- **Priority levels.** A small scale (high/normal/low) that sorts before the chosen sort. Less fiddly than dragging, but another axis to maintain per session.

Whichever it is, it has to work alongside the project/tag/time filters, the sort selector, and the search-filtered list, and be stored per machine like the other sidebar state.
