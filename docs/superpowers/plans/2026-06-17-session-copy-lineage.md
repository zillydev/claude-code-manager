# Session Copy/Fork Lineage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect when one session's history was copied into another (fork / resume / background-restart copies that share message uuids) and surface the link bidirectionally — sidebar badge + transcript banner — with the shared-message count shown.

**Architecture:** A pure core function `store::lineage::copy_lineage` infers, per project directory, each session's copy-ancestor from shared message uuids and directionality (a descendant's first message uuid appears *inside* the ancestor, and the two don't share the same first message). `store::discover` builds the per-directory inputs and writes two new `SessionSummary` fields (`copied_from`, `copied_overlap`). The frontend reads those: the sidebar mirrors the existing `forked_from` badge/hover linkage, and a new `LineageBanner` (rendered by `App` above `Transcript`, from the summaries list) jumps between ancestor and descendants.

**Tech Stack:** Rust (`std::collections::HashSet/HashMap`, `uuid`), React 19 + TypeScript.

## Global Constraints

- No new dependencies (runtime or dev).
- No changes to `claude-manager-core`'s public behavior beyond the additive `SessionSummary` fields and the new `lineage` module.
- Lineage detection is scoped **per project directory** (a copy keeps the same cwd → same encoded dir). No global/cross-directory index.
- `copied_from` is distinct from `forked_from`; when a session already has an explicit `forked_from`, it is **not** also given a `copied_from` (no double-link).
- Field names are snake_case to match existing `SessionSummary` serialization (`forked_from`, `total_tokens`).
- Commit messages end with the `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` trailer.

---

## File Structure

- **Create** `crates/core/src/store/lineage.rs` — `SessionUuids` input struct + pure `copy_lineage` function + unit tests.
- **Modify** `crates/core/src/store/mod.rs` — declare `pub mod lineage;`; restructure `discover`'s inner directory loop to retain each session's `(summary, SessionUuids)`, call `copy_lineage`, and set `copied_from`/`copied_overlap`. Add a discover integration test.
- **Modify** `crates/core/src/session/summary.rs` — add `copied_from: Option<String>` and `copied_overlap: Option<usize>` to `SessionSummary`; initialize both to `None` in `summarize`.
- **Modify** `crates/webui/frontend/src/types.ts` — add the two fields to `SessionSummary`.
- **Modify** `crates/webui/frontend/src/components/Sidebar.tsx` — add a `copiedChildrenBySource` map; descendant + ancestor badges; include copy edges in `relatedIds`.
- **Create** `crates/webui/frontend/src/components/LineageBanner.tsx` — the ancestor/descendant banner.
- **Modify** `crates/webui/frontend/src/App.tsx` — derive the open session's lineage from `sessions` and render `LineageBanner` above `Transcript`.

---

## Task 1: Core — `copy_lineage` pure detection (TDD)

**Files:**
- Create: `crates/core/src/store/lineage.rs`
- Modify: `crates/core/src/store/mod.rs` (add `pub mod lineage;` near the other module/use declarations at the top)
- Test: inline `#[cfg(test)]` in `lineage.rs`

**Interfaces:**
- Produces:
  - `pub struct SessionUuids { pub id: uuid::Uuid, pub head: Option<uuid::Uuid>, pub uuids: std::collections::HashSet<uuid::Uuid>, pub forked: bool }`
  - `pub fn copy_lineage(sessions: &[SessionUuids]) -> std::collections::HashMap<uuid::Uuid, (uuid::Uuid, usize)>` — maps a descendant session id to `(ancestor session id, shared-uuid count)`.

- [ ] **Step 1: Write the failing tests**

Create `crates/core/src/store/lineage.rs`:

```rust
//! Infer copy/fork lineage between sessions in the same project directory from
//! shared message uuids. When Claude Code forks/resumes/background-copies a
//! session it reuses the SAME message uuids for the copied portion, so a large
//! uuid overlap is the reliable lineage signal — Claude Code does not always
//! write an explicit `forkedFrom`.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

/// Per-session inputs for lineage detection.
pub struct SessionUuids {
    /// The session id (the `.jsonl` file stem).
    pub id: Uuid,
    /// The session's first message uuid in file order (its tree root). For a
    /// copy this is the fork point; for an origin it's its own earliest message.
    pub head: Option<Uuid>,
    /// All message uuids in the session.
    pub uuids: HashSet<Uuid>,
    /// Whether the session already has an explicit `forked_from` (skip — the
    /// explicit fork link already covers it).
    pub forked: bool,
}

/// For each non-explicitly-forked session `S`, find its copy-ancestor: another
/// session `A` that CONTAINS `S.head` as a non-root message (`A.head != S.head`),
/// which means `A` had that message before `S` was copied from it. Among
/// candidates, pick the largest uuid overlap (tie: larger uuid set, then smaller
/// id). Returns `descendant id -> (ancestor id, overlap count)`.
///
/// The `A.head != S.head` guard is what makes direction unambiguous and
/// cycle-free: if two sessions merely share the same starting message (siblings
/// or exact dupes) neither is treated as the other's ancestor.
pub fn copy_lineage(sessions: &[SessionUuids]) -> HashMap<Uuid, (Uuid, usize)> {
    let mut out = HashMap::new();
    for s in sessions {
        if s.forked {
            continue;
        }
        let Some(head) = s.head else { continue };
        // best = (overlap, ancestor_uuid_len, ancestor_id)
        let mut best: Option<(usize, usize, Uuid)> = None;
        for a in sessions {
            if a.id == s.id {
                continue;
            }
            if a.head == Some(head) {
                continue; // shared start => not an ancestor
            }
            if !a.uuids.contains(&head) {
                continue;
            }
            let overlap = s.uuids.intersection(&a.uuids).count();
            let cand = (overlap, a.uuids.len(), a.id);
            let better = match best {
                None => true,
                Some((bo, bl, bid)) => {
                    (overlap, a.uuids.len()) > (bo, bl)
                        || ((overlap, a.uuids.len()) == (bo, bl) && a.id < bid)
                }
            };
            if better {
                best = Some(cand);
            }
        }
        if let Some((overlap, _, ancestor_id)) = best {
            out.insert(s.id, (ancestor_id, overlap));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }
    fn set(items: &[u128]) -> HashSet<Uuid> {
        items.iter().map(|n| u(*n)).collect()
    }

    #[test]
    fn descendant_links_to_ancestor_containing_its_head() {
        // A = origin: head A1, also contains the shared message S.
        // B = copy:   head S (the fork point), plus its own new message B2.
        let a = SessionUuids { id: u(0xA), head: Some(u(1)), uuids: set(&[1, 5]), forked: false };
        let b = SessionUuids { id: u(0xB), head: Some(u(5)), uuids: set(&[5, 2]), forked: false };
        let links = copy_lineage(&[a, b]);
        assert_eq!(links.get(&u(0xB)), Some(&(u(0xA), 1)));
        assert_eq!(links.get(&u(0xA)), None);
    }

    #[test]
    fn no_overlap_no_link() {
        let a = SessionUuids { id: u(0xA), head: Some(u(1)), uuids: set(&[1, 2]), forked: false };
        let b = SessionUuids { id: u(0xB), head: Some(u(3)), uuids: set(&[3, 4]), forked: false };
        let links = copy_lineage(&[a, b]);
        assert!(links.is_empty());
    }

    #[test]
    fn explicit_fork_is_skipped() {
        // B would otherwise link to A, but it already has an explicit forked_from.
        let a = SessionUuids { id: u(0xA), head: Some(u(1)), uuids: set(&[1, 5]), forked: false };
        let b = SessionUuids { id: u(0xB), head: Some(u(5)), uuids: set(&[5, 2]), forked: true };
        let links = copy_lineage(&[a, b]);
        assert_eq!(links.get(&u(0xB)), None);
    }

    #[test]
    fn shared_start_is_not_a_link() {
        // Two siblings share the same head (5) and overlap, but neither contains
        // the other's head as a non-root, so neither is the other's ancestor.
        let a = SessionUuids { id: u(0xA), head: Some(u(5)), uuids: set(&[5, 1]), forked: false };
        let b = SessionUuids { id: u(0xB), head: Some(u(5)), uuids: set(&[5, 2]), forked: false };
        let links = copy_lineage(&[a, b]);
        assert!(links.is_empty());
    }

    #[test]
    fn descendant_larger_than_ancestor_still_links() {
        // Ancestor A is small (head A1 + shared S). Descendant B copied S then grew
        // bigger. Direction must come from head-containment, not size.
        let a = SessionUuids { id: u(0xA), head: Some(u(1)), uuids: set(&[1, 5]), forked: false };
        let b =
            SessionUuids { id: u(0xB), head: Some(u(5)), uuids: set(&[5, 2, 3, 4]), forked: false };
        let links = copy_lineage(&[a, b]);
        assert_eq!(links.get(&u(0xB)), Some(&(u(0xA), 1)));
        assert_eq!(links.get(&u(0xA)), None);
    }
}
```

Add the module declaration to `crates/core/src/store/mod.rs` (alongside the existing `pub mod paths;` / module declarations near the top):

```rust
pub mod lineage;
```

- [ ] **Step 2: Run the tests to verify they pass**

These tests exercise the implementation written in Step 1 (the function is included with the tests since it is small and pure). Run:

Run: `cargo test -p claude-manager-core lineage`
Expected: PASS — `descendant_links_to_ancestor_containing_its_head`, `no_overlap_no_link`, `explicit_fork_is_skipped`, `shared_start_is_not_a_link`, `descendant_larger_than_ancestor_still_links`.

(If you prefer strict red-first TDD: temporarily replace the `copy_lineage` body with `HashMap::new()`, run to see the four positive tests fail, then restore the body.)

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/store/lineage.rs crates/core/src/store/mod.rs
git commit -m "feat(core): copy_lineage — infer session copy-ancestry from shared uuids

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Core — `SessionSummary` fields + wire `discover`

**Files:**
- Modify: `crates/core/src/session/summary.rs` (struct + `summarize` initializer)
- Modify: `crates/core/src/store/mod.rs` (`discover` inner loop + a new test)

**Interfaces:**
- Consumes: `lineage::SessionUuids`, `lineage::copy_lineage` (Task 1); `Event::uuid()` → `Option<Uuid>` (existing).
- Produces: `SessionSummary.copied_from: Option<String>`, `SessionSummary.copied_overlap: Option<usize>`.

- [ ] **Step 1: Add the two fields to `SessionSummary`**

In `crates/core/src/session/summary.rs`, add to the struct (after `forked_from`, line ~24):

```rust
    /// Ancestor session this one's history was copied from, inferred from shared
    /// message uuids (fork / resume / background-restart copies that Claude Code
    /// does not record with an explicit `forkedFrom`). `None` when not a copy or
    /// when `forked_from` already links it. Resolved by `store::discover`.
    pub copied_from: Option<String>,
    /// Number of messages shared with `copied_from` (display: "N shared").
    pub copied_overlap: Option<usize>,
```

And in the `SessionSummary { … }` constructor returned by `summarize` (line ~127), add the two initializers after `forked_from`:

```rust
        forked_from,
        copied_from: None,
        copied_overlap: None,
```

- [ ] **Step 2: Verify existing core tests still compile/pass**

Run: `cargo test -p claude-manager-core summary`
Expected: PASS — all existing `summary::tests::*` still pass (they call `summarize`, which now sets the two new fields to `None`).

- [ ] **Step 3: Write the failing discover test**

Add to the `#[cfg(test)]` module of `crates/core/src/store/mod.rs` (create the module if none exists; if a tests module already exists, add this test into it). This builds a temp `~/.claude`-style root with two session files that share a message uuid:

```rust
#[cfg(test)]
mod lineage_discovery_tests {
    use super::*;
    use std::io::Write;

    fn write_session(dir: &std::path::Path, session_id: &str, lines: &[&str]) {
        let p = dir.join(format!("{session_id}.jsonl"));
        let mut f = std::fs::File::create(p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    #[test]
    fn discover_sets_copied_from_via_shared_uuid() {
        let tmp = std::env::temp_dir().join(format!("cm-lineage-test-{}", std::process::id()));
        let proj = tmp.join("projects").join("-tmp-proj");
        std::fs::create_dir_all(&proj).unwrap();

        // SHARED message uuid present in BOTH sessions.
        let shared = "55555555-0000-0000-0000-000000000005";
        // A (origin): own head A1, then the shared message.
        let a_id = "11111111-1111-1111-1111-111111111111";
        let a1 = "aaaaaaaa-0000-0000-0000-000000000001";
        write_session(
            &proj,
            a_id,
            &[
                &format!(r#"{{"type":"user","uuid":"{a1}","parentUuid":null,"isSidechain":false,"message":{{"role":"user","content":"a-first"}},"timestamp":"2026-06-17T00:00:00Z"}}"#),
                &format!(r#"{{"type":"user","uuid":"{shared}","parentUuid":"{a1}","isSidechain":false,"message":{{"role":"user","content":"shared"}},"timestamp":"2026-06-17T00:01:00Z"}}"#),
            ],
        );
        // B (copy): head = the shared message (its fork point), then a new message.
        let b_id = "22222222-2222-2222-2222-222222222222";
        let b2 = "bbbbbbbb-0000-0000-0000-000000000002";
        write_session(
            &proj,
            b_id,
            &[
                &format!(r#"{{"type":"user","uuid":"{shared}","parentUuid":null,"isSidechain":false,"message":{{"role":"user","content":"shared"}},"timestamp":"2026-06-17T00:02:00Z"}}"#),
                &format!(r#"{{"type":"user","uuid":"{b2}","parentUuid":"{shared}","isSidechain":false,"message":{{"role":"user","content":"b-new"}},"timestamp":"2026-06-17T00:03:00Z"}}"#),
            ],
        );

        let disc = discover(&tmp).unwrap();
        let a = disc.sessions.iter().find(|s| s.id.to_string() == a_id).unwrap();
        let b = disc.sessions.iter().find(|s| s.id.to_string() == b_id).unwrap();

        assert_eq!(b.copied_from.as_deref(), Some(a_id), "B should be copied from A");
        assert_eq!(b.copied_overlap, Some(1), "one shared message");
        assert_eq!(a.copied_from, None, "A is the origin");

        std::fs::remove_dir_all(&tmp).ok();
    }
}
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test -p claude-manager-core discover_sets_copied_from_via_shared_uuid`
Expected: FAIL — `copied_from` is `None` (discover does not yet resolve lineage).

- [ ] **Step 5: Wire lineage resolution into `discover`**

In `crates/core/src/store/mod.rs`, replace the inner per-file loop body so each directory accumulates `(SessionSummary, SessionUuids)`, then resolves lineage after the directory's files are read. Replace the existing inner `for child in inner { … }` block (the loop that parses each file and pushes a summary) with:

```rust
        // Accumulate this directory's sessions so we can resolve copy-lineage
        // (which needs every session's uuids) before pushing them.
        let mut dir_summaries: Vec<SessionSummary> = Vec::new();
        let mut dir_uuids: Vec<lineage::SessionUuids> = Vec::new();

        for child in inner {
            let child = match child {
                Ok(c) => c,
                Err(_) => continue,
            };
            let child_path = child.path();
            if child_path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let stem = match child_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            let id = match Uuid::parse_str(stem) {
                Ok(u) => u,
                Err(_) => continue, // skip non-UUID jsonl files
            };
            let mtime = child
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

            match parse_file(&child_path) {
                Ok(ParseOutcome { events, .. }) => {
                    let project_label = events
                        .iter()
                        .find_map(|e| e.cwd().map(paths::label_from_cwd))
                        .unwrap_or_else(|| fallback_label.clone());
                    let summary = summarize(
                        &events,
                        id,
                        child_path.clone(),
                        project_label,
                        mtime,
                    );
                    let su = lineage::SessionUuids {
                        id,
                        head: events.iter().find_map(|e| e.uuid()),
                        uuids: events.iter().filter_map(|e| e.uuid()).collect(),
                        forked: summary.forked_from.is_some(),
                    };
                    dir_summaries.push(summary);
                    dir_uuids.push(su);
                }
                Err(e) => {
                    warnings.push(format!("parse {}: {}", child_path.display(), e));
                }
            }
        }

        // Resolve copy-lineage within this directory and stamp the summaries.
        let links = lineage::copy_lineage(&dir_uuids);
        for summary in dir_summaries.iter_mut() {
            if let Some((ancestor_id, overlap)) = links.get(&summary.id) {
                summary.copied_from = Some(ancestor_id.to_string());
                summary.copied_overlap = Some(*overlap);
            }
        }
        sessions.extend(dir_summaries);
```

(Ensure `HashSet`/`HashMap` are available — `lineage::SessionUuids.uuids` collects into the `HashSet<Uuid>` declared in Task 1; the `.collect()` here infers it. No new `use` is required in `mod.rs` because the type is named via the field. If the compiler asks for it, add `use std::collections::HashSet;` — but the `.collect()` into the struct field infers the type.)

- [ ] **Step 6: Run the discover test + full core suite**

Run: `cargo test -p claude-manager-core`
Expected: PASS — `discover_sets_copied_from_via_shared_uuid` now passes; all existing core tests (61) + Task 1 lineage tests still pass.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/session/summary.rs crates/core/src/store/mod.rs
git commit -m "feat(core): resolve copied_from/copied_overlap per project dir in discover

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Frontend — types + sidebar linkage

**Files:**
- Modify: `crates/webui/frontend/src/types.ts`
- Modify: `crates/webui/frontend/src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: `SessionSummary.copied_from`, `SessionSummary.copied_overlap` (Task 2).

- [ ] **Step 1: Add the fields to the TS type**

In `crates/webui/frontend/src/types.ts`, add to the `SessionSummary` interface (after `forked_from`, line ~11):

```ts
  /// Ancestor session this one's history was copied from (shared-uuid lineage).
  copied_from: string | null;
  /// Number of messages shared with `copied_from`.
  copied_overlap: number | null;
```

- [ ] **Step 2: Add the copy-lineage maps in Sidebar**

In `crates/webui/frontend/src/components/Sidebar.tsx`, after the existing `childrenBySource` `useMemo` (ends ~line 56), add a parallel map for copies:

```tsx
  const copiedChildrenBySource = useMemo(() => {
    const m = new Map<string, SessionSummary[]>();
    for (const s of sessions) {
      if (s.copied_from) {
        const arr = m.get(s.copied_from);
        if (arr) arr.push(s);
        else m.set(s.copied_from, [s]);
      }
    }
    return m;
  }, [sessions]);
```

Then extend `relatedIds` so hovering highlights copy-relations too. Replace the existing `relatedIds` `useMemo` body with:

```tsx
  const relatedIds = useMemo(() => {
    if (!hoveredId) return null;
    const ids = new Set<string>();
    const h = byId.get(hoveredId);
    if (h?.forked_from) ids.add(h.forked_from);
    if (h?.copied_from) ids.add(h.copied_from);
    for (const c of childrenBySource.get(hoveredId) ?? []) ids.add(c.id);
    for (const c of copiedChildrenBySource.get(hoveredId) ?? []) ids.add(c.id);
    return ids.size ? ids : null;
  }, [hoveredId, byId, childrenBySource, copiedChildrenBySource]);
```

- [ ] **Step 3: Render the copy badges**

In the same row markup, mirror the fork badges. Find the `childrenBySource.has(s.id)` "⑂" badge block (ends ~line 350) and add, immediately after its closing `)}`:

```tsx
                    {copiedChildrenBySource.has(s.id) && (
                      <span
                        className="text-[var(--color-accent)] opacity-80"
                        title={`${copiedChildrenBySource.get(s.id)!.length} session(s) copied from this`}
                      >
                        ⧉ {copiedChildrenBySource.get(s.id)!.length}
                      </span>
                    )}
```

Then find the `{s.forked_from && ( … )}` block (the `⮌ from …` badge, ends ~line 372) and add, immediately after its closing `)}`:

```tsx
                  {s.copied_from && (
                    <div className="mt-1">
                      <span
                        onClick={(e) => {
                          e.stopPropagation();
                          const src = byId.get(s.copied_from!);
                          if (src) onSelect(src);
                        }}
                        title={
                          byId.has(s.copied_from)
                            ? "History copied from this session — click to open"
                            : `Copied from ${s.copied_from} (not in list)`
                        }
                        className={`inline-flex max-w-full items-center gap-1 truncate text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] ${
                          byId.has(s.copied_from)
                            ? "hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] cursor-pointer"
                            : "opacity-60"
                        }`}
                      >
                        ⮌ copied from {byId.get(s.copied_from)?.title ?? s.copied_from.slice(0, 8)}
                        {s.copied_overlap != null && ` · ${s.copied_overlap} shared`}
                      </span>
                    </div>
                  )}
```

- [ ] **Step 4: Type-check + run frontend tests**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npm test`
Expected: `tsc` clean; 14 frontend tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/webui/frontend/src/types.ts crates/webui/frontend/src/components/Sidebar.tsx
git commit -m "feat(webui): sidebar copy-lineage badges + hover-highlight

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Frontend — transcript lineage banner

**Files:**
- Create: `crates/webui/frontend/src/components/LineageBanner.tsx`
- Modify: `crates/webui/frontend/src/App.tsx`

**Interfaces:**
- Consumes: `SessionSummary` (with `copied_from`), the `sessions` list, and `transcript.id` (all in `App`).

- [ ] **Step 1: Create the banner component**

Create `crates/webui/frontend/src/components/LineageBanner.tsx`:

```tsx
import type { SessionSummary } from "../types";

interface Props {
  /** The ancestor this session was copied from, if any. */
  copiedFrom: SessionSummary | null;
  /** Sessions that were copied from this one. */
  copies: SessionSummary[];
  /** Shared-message count with the ancestor (for display). */
  overlap: number | null;
  onOpen: (s: SessionSummary) => void;
}

export default function LineageBanner({ copiedFrom, copies, overlap, onOpen }: Props) {
  if (!copiedFrom && copies.length === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-2 px-4 py-1.5 text-xs border-b border-[var(--color-border)] bg-[var(--color-surface)] shrink-0">
      {copiedFrom && (
        <button
          onClick={() => onOpen(copiedFrom)}
          className="inline-flex items-center gap-1 px-2 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors"
          title="Open the session this one's history was copied from"
        >
          ⮌ Copied from {copiedFrom.title}
          {overlap != null && ` — ${overlap} earlier messages live there`}
        </button>
      )}
      {copies.map((c) => (
        <button
          key={c.id}
          onClick={() => onOpen(c)}
          className="inline-flex items-center gap-1 px-2 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors"
          title="Open the session that was copied from this one"
        >
          Also continued as {c.title} →
        </button>
      ))}
    </div>
  );
}
```

- [ ] **Step 2: Wire it into App above the Transcript**

In `crates/webui/frontend/src/App.tsx`, add the import (after the `Transcript` import, line ~5):

```tsx
import LineageBanner from "./components/LineageBanner";
```

Replace the `<main>` block (lines ~101-113) with one that derives the open session's lineage from `sessions` and renders the banner above the transcript:

```tsx
      <main className="flex-1 overflow-hidden flex flex-col">
        {txLoading ? (
          <div className="flex items-center justify-center h-full text-[var(--color-text-dim)]">
            Loading session…
          </div>
        ) : transcript ? (
          (() => {
            const openSummary = sessions.find((s) => s.id === transcript.id) ?? null;
            const copiedFrom = openSummary?.copied_from
              ? sessions.find((s) => s.id === openSummary.copied_from) ?? null
              : null;
            const copies = sessions.filter((s) => s.copied_from === transcript.id);
            return (
              <>
                <LineageBanner
                  copiedFrom={copiedFrom}
                  copies={copies}
                  overlap={openSummary?.copied_overlap ?? null}
                  onOpen={(s) => open(s.file_path, s.project_label)}
                />
                <div className="flex-1 overflow-hidden">
                  <Transcript session={transcript} onBack={close} />
                </div>
              </>
            );
          })()
        ) : (
          <div className="flex items-center justify-center h-full text-[var(--color-text-dim)]">
            Select a session to view
          </div>
        )}
      </main>
```

- [ ] **Step 3: Type-check, test, build**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npm test && npm run build`
Expected: `tsc` clean; 14 tests PASS; `vite build` succeeds.

- [ ] **Step 4: Commit**

```bash
git add crates/webui/frontend/src/components/LineageBanner.tsx crates/webui/frontend/src/App.tsx
git commit -m "feat(webui): transcript banner linking copy ancestor + descendants

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Whole Rust suite**

Run: `cargo test`
Expected: all workspace tests PASS — existing 61 core + the 5 `lineage` tests + the discover lineage test + 3 webui + 3 TUI, 0 failures.

- [ ] **Step 2: Frontend gate**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npm test && npm run build`
Expected: `tsc` clean; 14 tests PASS; build succeeds.

- [ ] **Step 3: Manual verification (needs the running app — hand to the user; do not launch it)**

Provide these steps for the user to run after they launch `make webui`:
1. Open `91052458-9e2f-4cb9-80dc-afe3b9ce904d` → a banner reads **"⮌ Copied from `<81375432 title>` — 383 earlier messages live there"**; clicking it lands on `81375432`.
2. Open `81375432-9de4-41b1-9cee-c94721b5a9f2` → a banner reads **"Also continued as `<91052458 title>` →"**; clicking it lands on `91052458`.
3. In the sidebar, the `91052458` row shows **`⮌ copied from … · 383 shared`**, the `81375432` row shows **`⧉ 1`**, and hovering either highlights both.

- [ ] **Step 4: Report results**

Report exact `cargo test` and frontend test counts, plus the manual outcomes the user reports. Do not claim the manual checks pass without the user's confirmation. Do not merge — the user directs all merges.

---

## Notes for the implementer

- **Do not** double-link: a session with an explicit `forked_from` must not also get a `copied_from` (the `forked` flag in `SessionUuids` enforces this in Task 1).
- **Do not** touch `Transcript.tsx` (it was just perf-tuned with memoization); the banner lives in `App` so the transcript component is unchanged.
- Lineage is per-project-directory by construction (the `discover` loop is already per dir); never resolve across directories.
- The shared-uuid signal generalizes beyond compaction — it also catches `/branch` / `--fork-session` (which additionally carry `forked_from`, so those are skipped here) and background/restart copies (which do not). That's intended.
