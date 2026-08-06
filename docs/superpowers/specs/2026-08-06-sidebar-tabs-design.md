# claude-code-manager — Sidebar tabs (Obsidian-style)

**Date:** 2026-08-06 · **Status:** Approved
**Goal:** Replace the GlobalFind and BookmarkBrowser overlays with an icon tab strip at the top of the sidebar: 🗂 Sessions · ⚑ Bookmarks · 🔍 Find.

## Design

- **Strip** at the sidebar's top, always visible when expanded; active tab = accent + surface pill. Resize/collapse unchanged (collapse hides the whole sidebar, strip included).
- **Sessions** = the existing sidebar content, untouched, as the default pane.
- **Bookmarks / Find** = the existing components reshaped from modal to pane (backdrop, ✕, Escape-close, and the ⚑/🔍 header buttons removed). Clicking a hit/bookmark opens the session in the main pane and the pane STAYS — results persist next to what you opened.
- **Panes hide with `display:none`**, not unmount — search results and bookmark previews survive tab switches (the Manager/Terminal trick), which also makes lifting find state unnecessary.
- **State**: `sidebarTab` lives in App (single writer; shortcuts flow there). ⌘⇧F → find tab, expanding the sidebar if collapsed, focusing the input (query preserved). ⌘F → same, with `session:<id> ` prefilled via an `activate` nonce prop (`{q: string|null, n}`; q null = focus only) — a nonce because the always-mounted pane can't take a new value through `useState(initial)`.
- Sidebar's existing `onSelect(s, uuid?)` serves both panes' click-through.

## Testing

No new pure logic; suite stays green. Visual verification by the user (merge-gated).
