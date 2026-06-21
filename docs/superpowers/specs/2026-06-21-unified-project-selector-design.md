# Unified project/directory selector — design

**Date:** 2026-06-21
**Status:** Approved

## Problem

The sidebar has two filter dropdowns — **Projects** (by `project_label`, the
cwd's basename) and **Directories** (by full `cwd`). They're ~94% redundant: in
the real data only **2 of 33** project names map to more than one directory
(`integration-proxy`, `k8-kustomize` — same repo name in two places). For every
other project the two dropdowns are identical, so the Directories dropdown is
mostly clutter.

## Design

Replace the two dropdowns with **one selector** that groups by project name and
only reveals directories when a name **collides** (maps to >1 cwd).

### Options

- `All projects`
- For each distinct project name (basename), sorted:
  - **unique name** (≤1 distinct cwd) → a plain `<option>`, label = the name.
  - **colliding name** (>1 distinct cwd) → an `<optgroup label="<name>">` with one
    `<option>` per distinct cwd, labeled by `shortenPath(cwd)`. **No "All"
    option** — for a collision you pick a specific directory.

### State & filter

Replace the two states (`projectFilter`, `cwdFilter`) with one string `filter`:

- `""` → no filter.
- `name:<basename>` → keep sessions where `project_label === <basename>` (used by
  the unique-name options, so cwd-less sessions under that name still match).
- `dir:<cwd>` → keep sessions where `cwd === <cwd>` (used by the per-path options
  under a collision).

In the existing `filtered` memo, the project/cwd checks become:

```ts
if (filter.startsWith("name:") && s.project_label !== filter.slice(5)) return false;
if (filter.startsWith("dir:") && s.cwd !== filter.slice(4)) return false;
```

### Options data

Replace the `projects` and `cwds` memos with one `projectGroups` memo:
`Array<{ name: string; cwds: string[] }>` — sessions grouped by `project_label`,
each with its sorted distinct cwds (empty/undefined cwds dropped); the array
sorted by `name`. A group with `cwds.length <= 1` renders as a plain option
(value `name:<name>`); `> 1` renders as an optgroup of `dir:<cwd>` options.

### Untouched

The date-range dropdown, the search box, the text/search-hit filtering, and the
sort bar (incl. the "Project" sort by `project_label`) are unchanged. The
unified selector still ANDs with the date filter and search.

## Edge cases

- **cwd-less sessions under a colliding name:** they have no cwd, so they don't
  appear under any per-path option — reachable only via `All projects`.
  Acceptable (cwd-less sessions are rare; all current sessions have a cwd).

## Testing

- This is a `Sidebar.tsx`-only frontend change; the filtering logic is inline in
  the component (no separate testable unit). **tsc + existing vitest stay green.**
- **Manual (I won't launch the webui):** unique names show as a flat list (no
  paths); `integration-proxy` / `k8-kustomize` appear as optgroups with their two
  directories; selecting a name filters to it; selecting a collision's directory
  pins that exact path; `All projects` clears it; the filter ANDs with date +
  search.

## Non-goals

- No change to how `project_label` / `cwd` are computed in core.
- No "All (n)" aggregate option for collisions (decided: pick a specific dir).
- No persistence of the selection (matches today — the filters are ephemeral).
