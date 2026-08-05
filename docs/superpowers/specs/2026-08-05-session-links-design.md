# claude-code-manager — Session links panel

**Date:** 2026-08-05
**Status:** Approved (user delegated detail decisions)
**Goal:** "Some place to see all links from a session" — every URL that appeared in the conversation, one click away.

## Design

- Pure `extractLinks(turns)` in `lib/links.ts`: scans **user text, assistant text blocks, and command turns** — not tool results (a fetched page dumps hundreds of URLs; the conversation's own links are the ones that matter, and PR/issue URLs get echoed in assistant text anyway; revisit if practice disagrees).
- Bare URLs and markdown `[text](url)` both match; trailing punctuation (`.,;:!?`) and close-parens are stripped; deduped by URL keeping the **first** occurrence: `{ url, domain, uuid, role }`.
- UI: a header **🔗 n** dropdown (hidden at zero, like ⚑): each row shows the domain bold + the rest truncated; click **opens the URL externally** (`openUrl`); a small **⌖** per row jumps to the message instead (the bookmark jump helper, so narrowed/compact views widen automatically).
- Memoized on `session.turns`.

## Testing

Extraction: bare + markdown URLs, trailing-punctuation stripping, dedupe keeps first turn, attribution to the turn's head uuid, no matches → empty.

## Out of scope

Tool-result scanning, favicons, per-domain grouping, copy-all.
