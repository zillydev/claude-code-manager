# claude-code-manager — Discord-style find results

**Date:** 2026-08-14 · **Status:** Approved
**Goal:** Rework the find tab's results after Discord's search: count header, channel-style group headers, roomy message cards with the match highlighted.

## Design

- **Count bar** atop the results: `N results`, or `200+ results — narrow the query` when truncated (replaces the separate notice and "No matches" line — `0 results` covers it).
- **Group headers**: dim `#` + session title, `project · count` right-aligned dim. Grouping/order unchanged (newest session first, transcript order within).
- **Cards**: `role · time` header line (role colored, time dim), snippet body wrapping to `line-clamp-3`, click-through unchanged.
- **Match highlighting**: the run query's text wrapped in `--color-system` at ~35% alpha (amber, both themes) via a pure `splitHighlight(text, term)` in `lib/` (case-insensitive, all occurrences, original casing preserved). The term is stored WITH the results at Enter, so editing the input never shifts highlights under old results.
- **Snippet window** in `core::snippet_around` grows 30/50 → 60/120 chars (~180 total), sized for 2–3 wrapped lines at sidebar width.

## Out of scope

Pagination (cap + notice stays), Filters/Sort buttons (operators are the filters), avatars.
