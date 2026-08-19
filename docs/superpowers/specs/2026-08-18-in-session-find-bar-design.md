# claude-code-manager — In-session find bar (Zed-style)

**Date:** 2026-08-18 · **Status:** Approved
**Goal:** Bring back an in-session find — a Zed-style bar at the top of the transcript — for navigating the open session. ⌘F opens it (Zed's ⌘F buffer search / ⌘⇧F project search split); the global Find tab stays the deep, cross-session search and is one click away from the bar.

## Layout

A full-width strip between the session header and the message list (pushes content down, never floats), surface tone, bottom border:

```
[ Find in session ………………………… ]  Aa  ab̲  .*  │  ⚒  │  ‹  ›  3/27  │  ↗  ✕
```

- **Input** (flex-1): Enter → next, ⇧Enter → prev, Esc → close. ⌘F while open refocuses + selects the text. If the transcript has a non-empty text selection when ⌘F is pressed, it becomes the query.
- **Aa / whole word / .\*** (Lucide `CaseSensitive`, `WholeWord`, `Regex`): active = accent colour + subtle surface bg (the sidebar tab-strip treatment). An invalid regex replaces the counter with a red "Invalid regex".
- **⚒ Include tool calls & results** (Lucide `Wrench`): off → collapsed tool input/output and task results are not searched (the visible tool header line always is). On → they count toward n/N; stepping onto one auto-expands that call; reveals persist until the bar closes (or the toggle is switched off) so you can step back and forth. Manual expansions are never touched.
- **‹ › n/N** (Lucide `ChevronLeft/Right`): dim "0/0" when empty. ⌘G / ⌘⇧G step next/prev while the bar is open, even with focus in the transcript. No editable match-number field (Zed doesn't; dropped).
- **↗ Search all sessions** (Lucide `Globe`): opens the global Find tab with `session:<id> <query>` — the bridge that replaces today's ⌘F behaviour.
- **✕** close.
- The four toggles persist (`usePersistentState`, key `find.opts`) like Zed's search options; the query persists while the session stays open (close keeps it, reopen selects it).

## Match engine

One ordered list of `FindMatch { segId, turnIndex, blockIndex, field, start, end }` from two sources:

- **Rendered text** (always): walk the transcript's `[data-seg-id]` elements and match on `textContent`, so matches span markdown formatting and syntax-highlighted code. Fields: `user-text`, `assistant-text`, `system`, `command`, `summary`, `tool-name`, `task-summary`.
- **Collapsed bodies** (tools toggle on): `tool-input` (`JSON.stringify(input_full, null, 2)`), `tool-result` (`prettyResultText(result.text)`), `task-result` — computed from the data model with the *same* transforms the renderer applies, so offsets line up once the element is revealed. These fields come from the model whether or not the body is currently expanded (no DOM/model dedupe needed).
- Order: document position — `(turnIndex, blockIndex, field rank, start)`.
- Matcher: `compileMatcher(query, { caseSensitive, wholeWord, regex })` → `{ ok: true, find(text) → Span[] } | { ok: false, error }`. Literal mode escapes the query; whole-word wraps in `\b…\b`; the regex always carries `g` (+`i` unless case-sensitive); zero-length matches are skipped. Pure, unit-tested.
- Recompute on query / option / toggle change, on `turns` change (live tail), and after a reveal commits. State updates use functional setters that bail when already empty — the guard that fixed the turn-filter render loop.

## Painting & scrolling

- CSS Custom Highlight API: `CSS.highlights.set("cm-find", …)` / `"cm-find-active"`. No DOM mutation, so React re-renders can't lose or duplicate marks. Offsets → `Range` via a text-node walker over the segment element.
- Rules served raw from `index.html` (lightningcss strips `::highlight()`): all matches `color-mix(in srgb, var(--color-system) 35%, transparent)`, the active one 75% — tint only, text colour untouched (the global-find lesson: solid backgrounds made text unreadable).
- Painting runs in a layout effect keyed on `[matches, activeIndex, revealedKeys]`, i.e. *after* an auto-expand has committed, so a revealed tool body is highlighted on the frame it appears. The active range scrolls into view centered (cascading nested scrollers — tool bodies have their own `overflow-y: auto`).
- If the Highlight API is missing, the bar still counts and scrolls; nothing is painted.

## Keybindings

| Key | Action |
|---|---|
| ⌘F | Open / refocus the bar (Manager tab visible only; no-op when the Terminal tab is active) |
| ⌘⇧F | Global Find tab (unchanged) |
| ⌘G / ⌘⇧G | Next / previous match while the bar is open |
| Enter / ⇧Enter | Next / previous (input focused) |
| Esc | Close (input focused) |

Compact mode: find searches what is rendered — the buffer you're looking at.

## Files

- New: `components/FindBar.tsx`, `hooks/useFind.ts`, `lib/findMatcher.ts` (+test), `lib/findMatches.ts` (model-side matches, ordering; +test), `lib/findPaint.ts` (ranges, highlights, scroll).
- Touched: `Transcript.tsx` (mount the bar under the header, ⌘F/⌘G, pass `forceExpand` to `ToolCall`/`TaskBlock` from revealed keys — the prop survived the old bar's removal), `App.tsx` (`openFindTab(q)` already takes a query; Transcript's `onOpenFind(sessionId, query)`), `index.html` (highlight rules), `FEATURES.md` at merge.

## Not doing

Editable match-number field; Zed's replace / select-all-matches / search-in-selection (meaningless on a read-only transcript); per-role filters (`from:` in the global find covers it); searching `thinking` blocks (not rendered).

## Testing

Pure: matcher (case / word / regex / invalid / zero-length), model-side match collection + ordering (tool bodies included only with the toggle; task results; same transforms as the renderer). DOM painting and the bar itself are verified by the user in the app (merge-gated).
