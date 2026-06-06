# In-session find — design

Date: 2026-06-05
Status: Approved (design phase complete)

## Summary

A text-editor-style **find bar** for the currently open transcript in the webui.
It is **read-only** — it never mutates a session (editing/forking/deleting already
cover mutation). It is **entirely frontend**: the whole `RenderedSession` is already
in memory, so finding, counting, navigating, and highlighting need no Rust changes.

This is the in-session analogue of the existing cross-session `search_transcripts`
(which finds *which sessions* match a query). The two are independent and unrelated;
this feature finds *where in one session* a query matches.

## Goals

- Find across a single session's rendered content with live, incremental matching.
- Editor-grade controls: case-sensitive, whole-word, and regex toggles.
- Highlight all matches, distinguish the active match, navigate next/prev with wrap.
- Exact, complete match counts — including content that is collapsed by default.
- Correct highlighting over markdown-rendered assistant text, including matches that
  span markdown formatting boundaries (e.g. across `**bold**`).

## Non-goals

- No find-and-replace (read-only feature; replace was explicitly deferred).
- No search of thinking blocks (they are not rendered in the UI).
- No cross-session search here (that already exists in the sidebar).
- No backend / Rust / edit-pipeline changes.

## UX & controls

- **Open:** `Cmd/Ctrl+F` while a transcript is open opens a floating find bar at the
  top-right of the transcript pane (below the header), with the input autofocused.
  The handler calls `preventDefault` so the browser's native find does not appear.
- **Close:** `Esc`, or a `×` button. Closing clears highlights and restores Compact
  mode if it was on before find opened.
- **Live search:** matches recompute as the user types, debounced ~100ms.
- **Match count:** `3 / 47` (active / total); `No results` when a non-empty query has
  zero matches; nothing shown when the query is empty.
- **Navigate:** `Enter` = next, `Shift+Enter` = prev; `▲` / `▼` buttons mirror this.
  Navigation wraps around. The active match is scrolled to center.
- **Toggles:**
  - `Aa` — case-sensitive (default off: case-insensitive).
  - `\b` — whole-word (word-boundary anchored).
  - `.*` — regex (interpret the query as a JS regular expression).
  - Invalid regex → the bar shows an error state and reports zero matches.
- **Active vs other matches:** the active match is painted in an accent/orange color;
  all other matches use a dimmer yellow.

## Coverage

Searched:

- user message text
- assistant markdown text blocks
- system lines
- compaction summaries
- tool **name**
- tool **input** (the full JSON, exactly as rendered: `JSON.stringify(input, null, 2)`)
- tool **result** text

Not searched:

- thinking blocks (not rendered in the UI, so nothing to highlight)

Reveal behavior:

- **Collapsed tool calls** are *counted* always (their input/result are exact stored
  strings, so they can be matched without being mounted). The tool call you *navigate
  to* auto-expands so the hit is visible.
- **Compact mode:** opening find **suspends** compact (renders all turns) so counts are
  exact and complete and every hit is reachable. Closing find restores the prior compact
  state. (Chosen over "stay in compact, reveal on navigate" because markdown prose in a
  compact-hidden turn cannot be exactly counted without mounting it; suspending compact
  keeps the count honest, which is the stated intent of including compact-hidden turns.)
- **Live tail:** if new turns stream in (`session-updated`) while find is open, matches
  recompute and `activeIndex` is clamped to remain in range.

## Architecture

All new code is frontend. Files:

### `lib/search.ts` — pure matcher (TDD'd)

```
compileMatcher(query: string, opts: { caseSensitive: boolean; wholeWord: boolean; regex: boolean })
  -> { ok: true, find: (text: string) => Array<{ start: number; end: number }> }
   | { ok: false, error: string }
```

- Plain mode: literal substring search (case-insensitive by default).
- `wholeWord`: anchor matches at word boundaries.
- `regex`: compile the query as a JS `RegExp`; invalid pattern → `{ ok: false }`.
- Internally uses a global-flag regex to enumerate all matches; **guards the
  zero-width case** (e.g. `a*`, `^`, empty match) by advancing `lastIndex` by at least
  one to prevent an infinite loop. Returns matches in ascending offset order.
- Pure and deterministic → unit-tested with **vitest** (this is the project's first JS
  test runner; add `vitest` as a devDependency and a `test` script).

### Two match sources, both exact-by-construction

The match list is assembled per **segment**. A segment is one searchable region with a
stable id and a known document-order rank.

- **Exact segments** — user text, system, summary, tool name, tool input, tool result.
  Matched directly off the data-model string. The rendered `textContent` equals that
  string verbatim:
  - user text: rendered in a `whitespace-pre-wrap` div → `textContent === text`.
  - system / summary: same.
  - tool name: span text is `name` (the rendered span also has a trailing `:`, which is
    simply unmatched trailing text — harmless because matching is per-segment-element).
  - tool input `<pre>`: content is `JSON.stringify(input, null, 2)`, identical to the
    data-model `JSON.stringify(input_full, null, 2)`.
  - tool result `<pre>`: content is `result.text` verbatim.

  Because source string === rendered `textContent`, char offsets map 1:1 to DOM Ranges.
  These segments are **countable even while collapsed/unmounted**.

- **Markdown segments** — assistant text blocks. Matched off the **live rendered
  `textContent`** (always mounted, since compact is suspended while find is open).
  Offsets come from the very nodes that get painted, so it is self-consistent and
  handles matches spanning markdown formatting boundaries natively (a `Range` can span
  multiple text nodes / elements).

### `lib/highlight.ts` — painting (CSS Custom Highlight API)

`paintHighlights(container, matches, activeId)`:

- Iterate `container.querySelectorAll('[data-seg-id]')`. For **each element**, walk only
  that element's own descendant text nodes (never a single global `TreeWalker` over the
  container — that would sweep in the collapsed tool header's `▸`/`▾` triangle, `name:`,
  and truncated `input_summary` and corrupt offsets). Seg-ids do not nest, so per-element
  walking is unambiguous.
- For each match belonging to that segment, convert `[start, end)` char offsets into a
  DOM `Range` over the element's text nodes.
- Register two highlight groups: `CSS.highlights.set('cm-find', allRangesHighlight)` and
  `CSS.highlights.set('cm-find-active', activeRangeHighlight)`.
- **Feature-detect** `CSS.highlights`: if absent, skip painting and fall back to
  scroll-only navigation with a brief ring flash on the active segment. (Darwin 25.x ships
  WKWebView/Safari well past the 17.2 floor, so the API is available in practice; the
  fallback is for portability.)

### `hooks/useFind.ts` — state & navigation

Owns: `open`, `query`, `opts` (case/word/regex), ordered `matches[]`, `activeIndex`.
Exposes: `open()`, `close()`, `next()`, `prev()`, `setQuery()`, `setOpts()`.

- Match order is document order: `(turnIndex, blockIndex, fieldRank, offset)`.
- Stable `data-seg-id` encodes location, e.g. `5:2:tool-input`, `3:-:user-text`.
  `fieldRank` orders sub-fields within a tool_call: name=0, input=1, result=2.

### `components/FindBar.tsx` — the bar UI

Input, count readout, prev/next buttons, the three toggles, close button. Wired to
`useFind`.

### Wiring: `Transcript.tsx` / `ToolCall.tsx`

- Add `data-seg-id` to every searchable element (user text div, assistant markdown
  wrapper div, system/summary divs, tool name span, tool input `<pre>`, tool result
  `<pre>`).
- `ToolCall` gains a controlled `forceExpand?: boolean` prop driven by the reveal set so
  the navigated-to tool call expands.
- Suspend compact while find is open (track and restore prior state).
- **Paint runs in a `useLayoutEffect`** keyed on `(matches, activeIndex, revealState)` so
  it fires *after* the reveal/expand re-render commits (the `<pre>` is mounted by then),
  then scrolls the active range to center. Painting synchronously in the navigate click
  handler would target a not-yet-mounted node.
- `Cmd/Ctrl+F` / `Esc` keyboard handling lives in `Transcript` (only active when a
  transcript is shown).

### CSS (`index.css`)

```
::highlight(cm-find)        { background: <dim yellow>; }
::highlight(cm-find-active) { background: <accent/orange>; color: <bg>; }
```

## Testing

- **Automated (vitest):** `lib/search.ts` — plain match, case-insensitive default,
  case-sensitive, whole-word, regex, invalid regex → error, zero-width guard (`a*` does
  not hang), multi-match ascending order.
- **Manual** (jsdom implements neither the Custom Highlight API nor layout —
  `getBoundingClientRect`/`scrollIntoView` are no-ops — so these cannot be meaningfully
  unit-tested): paint correctness across markdown and `<pre>`, a match spanning a markdown
  boundary, navigate auto-expands a collapsed tool call, scroll-to-center, compact
  suspend/restore, live-tail recompute, and the feature-detect fallback path.

## Risks / edge cases

- **Markdown `textContent` vs source divergence** is sidestepped: markdown is matched off
  the live DOM, never off a separately-stripped text model. Exact segments are matched off
  strings that equal their rendered text verbatim.
- **Zero-width regex** could infinite-loop the match enumerator — explicitly guarded.
- **Reveal timing** (expand/compact-suspend then paint) is handled by painting in
  `useLayoutEffect` after commit, not in the event handler.
- **Live tail** mutating the turn list mid-find is handled by recompute + `activeIndex`
  clamp.
- **Browser support** for `CSS.highlights` is feature-detected with a graceful fallback.
```
