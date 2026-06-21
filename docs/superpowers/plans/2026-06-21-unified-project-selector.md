# Unified Project Selector Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the sidebar's two filter dropdowns (Projects + Directories) with one selector that groups by project name and only expands to per-directory options when a name collides.

**Architecture:** One `filter` string state (`""` | `name:<basename>` | `dir:<cwd>`) replaces `projectFilter`/`cwdFilter`; a `projectGroups` memo groups sessions by `project_label` with their distinct cwds; the dropdown renders a plain option per unique name and an `<optgroup>` of `dir:` options per colliding name. `Sidebar.tsx`-only.

**Tech Stack:** React/TS.

## Global Constraints

- Selector value encodes the filter kind: `name:<basename>` matches `project_label`; `dir:<cwd>` matches `cwd`; `""` = all.
- Collisions (>1 distinct cwd for a name) expand to per-directory `dir:` options — **no "All" aggregate** option.
- Date filter, search box, text/search-hit filtering, and the sort bar are untouched.
- Commit messages end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## Task 1: Unify the selector in `Sidebar.tsx`

**Files:**
- Modify: `crates/webui/frontend/src/components/Sidebar.tsx`

- [ ] **Step 1: Replace the two filter states with one**

Replace these two lines (currently ~27–28):

```tsx
  const [projectFilter, setProjectFilter] = useState<string | null>(null);
  const [cwdFilter, setCwdFilter] = useState<string | null>(null);
```

with:

```tsx
  // One project/directory filter: "" | "name:<basename>" | "dir:<cwd>".
  const [filter, setFilter] = useState<string>("");
```

- [ ] **Step 2: Replace the `projects`/`cwds` memos with `projectGroups`**

Replace this block (currently ~81–90):

```tsx
  // Unique project labels and cwds for dropdown options.
  const projects = useMemo(
    () => [...new Set(sessions.map((s) => s.project_label))].sort(),
    [sessions]
  );
  const cwds = useMemo(
    () =>
      [...new Set(sessions.map((s) => s.cwd).filter(Boolean) as string[])].sort(),
    [sessions]
  );
```

with:

```tsx
  // Sessions grouped by project name (basename), each with its distinct cwds.
  // A group with >1 cwd is a "collision" and expands to per-directory options.
  const projectGroups = useMemo(() => {
    const byName = new Map<string, Set<string>>();
    for (const s of sessions) {
      const set = byName.get(s.project_label) ?? new Set<string>();
      if (s.cwd) set.add(s.cwd);
      byName.set(s.project_label, set);
    }
    return [...byName.entries()]
      .map(([name, cwds]) => ({ name, cwds: [...cwds].sort() }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [sessions]);
```

- [ ] **Step 3: Update the filter memo (checks + deps)**

In the `filtered` memo, replace these two checks (currently ~131–134):

```tsx
      // Project filter
      if (projectFilter && s.project_label !== projectFilter) return false;
      // CWD filter
      if (cwdFilter && s.cwd !== cwdFilter) return false;
```

with:

```tsx
      // Project / directory filter (one selector).
      if (filter.startsWith("name:") && s.project_label !== filter.slice(5)) return false;
      if (filter.startsWith("dir:") && s.cwd !== filter.slice(4)) return false;
```

and in that memo's dependency array, replace `projectFilter, cwdFilter` with `filter`:

```tsx
  }, [sessions, filter, dateRange, textFilter, searchHits, now]);
```

- [ ] **Step 4: Replace the two `<select>`s with the unified one**

Replace the two `<select>` blocks (the "All projects" select and the "All directories" select, currently ~223–247) with this single select:

```tsx
          <select
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            className="bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-1.5 py-1 rounded border border-[var(--color-border)] outline-none max-w-40 truncate"
          >
            <option value="">All projects</option>
            {projectGroups.map((g) =>
              g.cwds.length > 1 ? (
                <optgroup key={g.name} label={g.name}>
                  {g.cwds.map((c) => (
                    <option key={c} value={`dir:${c}`}>
                      {shortenPath(c)}
                    </option>
                  ))}
                </optgroup>
              ) : (
                <option key={g.name} value={`name:${g.name}`}>
                  {g.name}
                </option>
              )
            )}
          </select>
```

(The date-range `<select>` immediately after stays as-is.)

- [ ] **Step 5: Typecheck + tests**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npx vitest run`
Expected: tsc clean (no remaining references to `projectFilter`/`cwdFilter`/`projects`/`cwds`); existing 20 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/webui/frontend/src/components/Sidebar.tsx
git commit -m "feat(webui): unify project + directory filters into one selector

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Verification

**Files:** none.

- [ ] **Step 1: Frontend checks + build**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npx vitest run && npm run build`
Expected: tsc clean; 20 tests; vite build succeeds.

- [ ] **Step 2: Hand the user manual steps (I won't launch the webui)**

  1. The sidebar now has **one** project dropdown (Directories is gone).
  2. Unique projects show as a flat list of names (no paths) — e.g. `claude-manager`, `mono`.
  3. `integration-proxy` and `k8-kustomize` appear as **optgroups**, each listing their two directories (`shortenPath`).
  4. Selecting a name filters to that project; selecting a collision's directory pins that exact path; `All projects` clears it.
  5. The filter still ANDs with the date dropdown and the search box; the Project sort still works.

- [ ] **Step 3: Report**

Report tsc/test/build results + the manual steps. Do not merge — the user directs merges.

---

## Notes for the implementer

- After the edits, grep `Sidebar.tsx` for `projectFilter`, `cwdFilter`, `projects`, `cwds` to confirm no stragglers remain (tsc will also catch them).
- `shortenPath` already exists in the file (used by the old directories select) — reused as-is.
- `filter.slice(5)` strips `"name:"` (5 chars); `filter.slice(4)` strips `"dir:"` (4 chars).
