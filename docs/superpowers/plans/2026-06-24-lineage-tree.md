# Lineage Tree Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An on-demand overlay that shows a session's whole fork/copy family as an indented tree, with click-to-jump navigation.

**Architecture:** The tree structure is built on the frontend from the `sessions` list (each session has ≤1 parent). A small backend command resolves the branch-message text snippets (which live in parent `.jsonl` files). An overlay component renders the indented tree.

**Tech Stack:** Rust (core parse + Tauri command) + React/TS/Tailwind. Tests: cargo + vitest.

## Global Constraints

- **Reuse existing lineage data** (`forked_from`/`copied_from`/`*_from_message`/`copied_overlap`); no new lineage computation.
- **Tree structure is frontend-only**; only branch-message *snippets* need the backend.
- A session with **no lineage** never shows a trigger.
- Cycle-guard the tree walk (defensive).
- Verification: `cargo test`, `tsc --noEmit`, `vitest run`, `vite build` all green.

---

### Task 1: Backend — message snippet resolution + command

**Files:**
- Modify: `crates/core/src/store/mod.rs`
- Modify: `crates/webui/src/commands.rs`
- Modify: `crates/webui/src/lib.rs`

**Interfaces:**
- Produces: `store::message_snippets(&Path, &HashSet<Uuid>) -> HashMap<Uuid, String>`; command `resolve_branch_messages(refs: Vec<BranchRef>) -> Vec<BranchSnippet>`.

- [ ] **Step 1: Write the failing core test**

In `crates/core/src/store/mod.rs` tests module, add (mirrors the existing temp-file style):

```rust
    #[test]
    fn message_snippets_resolves_text_and_truncates() {
        use std::collections::HashSet;
        let root = std::env::temp_dir().join(format!("cm-snip-{}", std::process::id()));
        let proj = root.join("projects").join("-tmp-proj");
        fs::create_dir_all(&proj).unwrap();
        let path = proj.join("s.jsonl");
        let u1 = "00000000-0000-0000-0000-0000000000a1";
        let u2 = "00000000-0000-0000-0000-0000000000a2";
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                format!(r#"{{"type":"user","uuid":"{u1}","message":{{"role":"user","content":"hello fork point here"}}}}"#),
                format!(r#"{{"type":"user","uuid":"{u2}","message":{{"role":"user","content":"{}"}}}}"#, "x ".repeat(80)),
            ),
        )
        .unwrap();

        let wanted: HashSet<Uuid> =
            [u1, u2].iter().map(|s| Uuid::parse_str(s).unwrap()).collect();
        let got = message_snippets(&path, &wanted);

        assert_eq!(got.get(&Uuid::parse_str(u1).unwrap()).map(String::as_str), Some("hello fork point here"));
        let long = got.get(&Uuid::parse_str(u2).unwrap()).unwrap();
        assert!(long.ends_with('…') && long.chars().count() == 60, "got: {long:?}");
        // An unknown uuid is absent.
        assert!(!got.contains_key(&Uuid::parse_str("00000000-0000-0000-0000-0000000000ff").unwrap()));

        let _ = fs::remove_dir_all(&root);
    }
```

- [ ] **Step 2: Run it, verify it fails**

Run: `cargo test -p claude-manager-core message_snippets`
Expected: FAIL — `cannot find function message_snippets`.

- [ ] **Step 3: Implement `message_snippets`**

In `crates/core/src/store/mod.rs`: extend the imports —

```rust
use std::collections::{HashMap, HashSet};
```
```rust
use crate::session::event::{Block, Event, MessageContent, UserBlock};
```

(Keep the existing `use` lines; add these.) Then add the functions (near `scan_activity`):

```rust
/// One-line, ~`max`-char previews for specific message uuids in a single session
/// file. Parses the file once; returns only requested uuids that resolve to a
/// non-empty text turn (a user text message, or an assistant's first text block).
/// Used to label fork/copy branch points in the lineage tree.
pub fn message_snippets(path: &Path, wanted: &HashSet<Uuid>) -> HashMap<Uuid, String> {
    let mut out = HashMap::new();
    if wanted.is_empty() {
        return out;
    }
    let outcome = match parse_file(path) {
        Ok(o) => o,
        Err(_) => return out,
    };
    for ev in &outcome.events {
        let id = match ev.uuid() {
            Some(id) => id,
            None => continue,
        };
        if !wanted.contains(&id) || out.contains_key(&id) {
            continue;
        }
        if let Some(text) = preview_text(ev) {
            let line = one_line_truncated(&text, 60);
            if !line.is_empty() {
                out.insert(id, line);
            }
        }
    }
    out
}

fn preview_text(ev: &Event) -> Option<String> {
    match ev {
        Event::User(u) => match &u.message.content {
            MessageContent::Text(t) => Some(t.clone()),
            MessageContent::Blocks(bs) => bs.iter().find_map(|b| match b {
                UserBlock::Text { text } => Some(text.clone()),
                _ => None,
            }),
        },
        Event::Assistant(a) => a.message.content.iter().find_map(|b| match b {
            Block::Text { text } => Some(text.clone()),
            _ => None,
        }),
        _ => None,
    }
}

fn one_line_truncated(s: &str, max: usize) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        let t: String = collapsed.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}
```

- [ ] **Step 4: Run it, verify it passes**

Run: `cargo test -p claude-manager-core message_snippets`
Expected: PASS.

- [ ] **Step 5: Add the `resolve_branch_messages` command**

In `crates/webui/src/commands.rs`, add (near the other commands):

```rust
#[derive(serde::Deserialize)]
pub struct BranchRef {
    pub file_path: String,
    pub message_uuid: String,
}

#[derive(Serialize)]
pub struct BranchSnippet {
    pub message_uuid: String,
    pub snippet: String,
}

/// Resolve short previews for fork/copy branch-point messages. Each branch
/// message lives in a parent session's file; groups by file so each parent is
/// parsed once. Unresolvable uuids are simply omitted. Off the UI thread.
#[tauri::command]
pub async fn resolve_branch_messages(
    refs: Vec<BranchRef>,
) -> Result<Vec<BranchSnippet>, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<BranchSnippet>, String> {
        use std::collections::{HashMap, HashSet};
        let mut by_file: HashMap<String, HashSet<uuid::Uuid>> = HashMap::new();
        for r in &refs {
            if let Ok(id) = uuid::Uuid::parse_str(&r.message_uuid) {
                by_file.entry(r.file_path.clone()).or_default().insert(id);
            }
        }
        let mut snippets: HashMap<uuid::Uuid, String> = HashMap::new();
        for (file, wanted) in &by_file {
            snippets.extend(store::message_snippets(std::path::Path::new(file), wanted));
        }
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for r in &refs {
            if let Ok(id) = uuid::Uuid::parse_str(&r.message_uuid) {
                if seen.insert(id) {
                    if let Some(s) = snippets.get(&id) {
                        out.push(BranchSnippet {
                            message_uuid: r.message_uuid.clone(),
                            snippet: s.clone(),
                        });
                    }
                }
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}
```

- [ ] **Step 6: Register it**

In `crates/webui/src/lib.rs` `generate_handler![...]`, add after `commands::refresh_sessions,`:

```rust
            commands::refresh_sessions,
            commands::resolve_branch_messages,
```

- [ ] **Step 7: Build + test the workspace**

Run: `cargo test && cargo build -p claude-manager-webui`
Expected: all green; webui builds (fix any unused imports).

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/store/mod.rs crates/webui/src/commands.rs crates/webui/src/lib.rs
git commit -m "feat(webui): resolve_branch_messages command + core message_snippets"
```

---

### Task 2: Frontend — `buildLineageTree` (pure)

**Files:**
- Create: `crates/webui/frontend/src/lib/lineageTree.ts`
- Create: `crates/webui/frontend/src/lib/lineageTree.test.ts`

**Interfaces:**
- Produces: `buildLineageTree(sessions, centerId) -> LineageNode | null`; types `LineageNode`, `LineageEdge`.

- [ ] **Step 1: Write the failing test**

Create `crates/webui/frontend/src/lib/lineageTree.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { buildLineageTree } from "./lineageTree";
import type { SessionSummary } from "../types";

const mk = (over: Partial<SessionSummary> & { id: string }): SessionSummary => ({
  id: over.id,
  file_path: `/p/${over.id}.jsonl`,
  project_label: "proj",
  cwd: null,
  git_branch: null,
  title: over.id,
  mtime: { secs_since_epoch: 0, nanos_since_epoch: 0 },
  last_event_at: null,
  awaiting_input: false,
  total_tokens: 0,
  forked_from: null,
  copied_from: null,
  copied_overlap: null,
  forked_from_message: null,
  copied_from_message: null,
  ...over,
});

describe("buildLineageTree", () => {
  it("roots from a deep child and classifies fork vs copy edges", () => {
    const root = mk({ id: "root" });
    const fork = mk({ id: "fork", forked_from: "root", forked_from_message: "m1" });
    const copy = mk({ id: "copy", copied_from: "root", copied_from_message: "m2", copied_overlap: 5 });
    const grand = mk({ id: "grand", forked_from: "fork", forked_from_message: "m3" });

    const tree = buildLineageTree([grand, copy, fork, root], "grand")!;
    expect(tree.session.id).toBe("root");
    expect(tree.edge).toBe("root");
    const ids = tree.children.map((c) => c.session.id).sort();
    expect(ids).toEqual(["copy", "fork"]);
    const forkNode = tree.children.find((c) => c.session.id === "fork")!;
    expect(forkNode.edge).toBe("forked");
    expect(forkNode.branchMessageUuid).toBe("m1");
    expect(forkNode.branchParentFilePath).toBe("/p/root.jsonl");
    expect(forkNode.children[0].session.id).toBe("grand");
    const copyNode = tree.children.find((c) => c.session.id === "copy")!;
    expect(copyNode.edge).toBe("copied");
    expect(copyNode.copiedOverlap).toBe(5);
  });

  it("returns null for an unknown center", () => {
    expect(buildLineageTree([mk({ id: "a" })], "nope")).toBeNull();
  });

  it("does not hang on a cycle", () => {
    const a = mk({ id: "a", forked_from: "b" });
    const b = mk({ id: "b", forked_from: "a" });
    const tree = buildLineageTree([a, b], "a");
    expect(tree).not.toBeNull();
  });
});
```

- [ ] **Step 2: Run it, verify it fails**

Run: `cd crates/webui/frontend && npx vitest run lineageTree`
Expected: FAIL — cannot resolve `./lineageTree`.

- [ ] **Step 3: Implement**

Create `crates/webui/frontend/src/lib/lineageTree.ts`:

```ts
import type { SessionSummary } from "../types";

export type LineageEdge = "root" | "forked" | "copied";

export interface LineageNode {
  session: SessionSummary;
  edge: LineageEdge;
  /** The parent message uuid this session branched at (null for the root). */
  branchMessageUuid: string | null;
  /** The parent's file path, where that branch message lives (null for root). */
  branchParentFilePath: string | null;
  copiedOverlap: number | null;
  children: LineageNode[];
}

const parentId = (s: SessionSummary): string | null => s.forked_from ?? s.copied_from ?? null;

/** Build the whole fork/copy family of `centerId` as a tree rooted at the
 *  oldest ancestor. Cycle-guarded. Returns null if `centerId` isn't present. */
export function buildLineageTree(
  sessions: SessionSummary[],
  centerId: string,
): LineageNode | null {
  const byId = new Map(sessions.map((s) => [s.id, s]));
  if (!byId.has(centerId)) return null;

  // Walk up to the root.
  let rootId = centerId;
  const seenUp = new Set<string>([centerId]);
  for (;;) {
    const p = parentId(byId.get(rootId)!);
    if (!p || !byId.has(p) || seenUp.has(p)) break;
    seenUp.add(p);
    rootId = p;
  }

  // Index children by parent.
  const childrenOf = new Map<string, SessionSummary[]>();
  for (const s of sessions) {
    const p = parentId(s);
    if (p && byId.has(p)) {
      const arr = childrenOf.get(p);
      if (arr) arr.push(s);
      else childrenOf.set(p, [s]);
    }
  }

  const build = (s: SessionSummary, edge: LineageEdge, visited: Set<string>): LineageNode => {
    visited.add(s.id);
    const pid = parentId(s);
    const node: LineageNode = {
      session: s,
      edge,
      branchMessageUuid:
        edge === "forked" ? s.forked_from_message : edge === "copied" ? s.copied_from_message : null,
      branchParentFilePath: edge === "root" || !pid ? null : byId.get(pid)?.file_path ?? null,
      copiedOverlap: edge === "copied" ? s.copied_overlap : null,
      children: [],
    };
    for (const c of childrenOf.get(s.id) ?? []) {
      if (visited.has(c.id)) continue;
      const childEdge: LineageEdge = c.forked_from === s.id ? "forked" : "copied";
      node.children.push(build(c, childEdge, visited));
    }
    return node;
  };

  return build(byId.get(rootId)!, "root", new Set<string>());
}
```

- [ ] **Step 4: Run it, verify it passes + tsc**

Run: `cd crates/webui/frontend && npx vitest run lineageTree && npx tsc --noEmit`
Expected: tests pass; tsc clean.

- [ ] **Step 5: Commit**

```bash
git add crates/webui/frontend/src/lib/lineageTree.ts crates/webui/frontend/src/lib/lineageTree.test.ts
git commit -m "feat(webui): buildLineageTree pure helper (walk-to-root family tree)"
```

---

### Task 3: Frontend — `LineageTree` overlay

**Files:**
- Create: `crates/webui/frontend/src/components/LineageTree.tsx`

**Interfaces:**
- Consumes: `buildLineageTree`, `resolve_branch_messages`. Props `{ sessions, centerId, onOpen, onClose }`.

- [ ] **Step 1: Create the component**

```tsx
// crates/webui/frontend/src/components/LineageTree.tsx
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SessionSummary } from "../types";
import { buildLineageTree, type LineageNode } from "../lib/lineageTree";

export default function LineageTree({
  sessions,
  centerId,
  onOpen,
  onClose,
}: {
  sessions: SessionSummary[];
  centerId: string;
  onOpen: (s: SessionSummary) => void;
  onClose: () => void;
}) {
  const root = useMemo(() => buildLineageTree(sessions, centerId), [sessions, centerId]);
  const [snippets, setSnippets] = useState<Record<string, string>>({});

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    if (!root) return;
    const refs: { file_path: string; message_uuid: string }[] = [];
    const walk = (n: LineageNode) => {
      if (n.branchMessageUuid && n.branchParentFilePath) {
        refs.push({ file_path: n.branchParentFilePath, message_uuid: n.branchMessageUuid });
      }
      n.children.forEach(walk);
    };
    walk(root);
    if (refs.length === 0) return;
    invoke<{ message_uuid: string; snippet: string }[]>("resolve_branch_messages", { refs })
      .then((rows) => setSnippets(Object.fromEntries(rows.map((r) => [r.message_uuid, r.snippet]))))
      .catch(() => {});
  }, [root]);

  if (!root) return null;

  const renderNode = (node: LineageNode, depth: number) => {
    const isCurrent = node.session.id === centerId;
    const snippet = node.branchMessageUuid ? snippets[node.branchMessageUuid] : undefined;
    const label =
      node.edge === "root"
        ? "◀ origin"
        : node.edge === "forked"
          ? snippet
            ? `⑂ forked at “${snippet}”`
            : "⑂ forked here"
          : `⧉ copied${node.copiedOverlap != null ? ` · ${node.copiedOverlap} shared` : ""}`;
    return (
      <div key={node.session.id} style={{ paddingLeft: depth * 18 }}>
        <div className="mt-1.5 text-[10px] text-[var(--color-text-dim)]">{label}</div>
        <button
          disabled={isCurrent}
          onClick={() => !isCurrent && onOpen(node.session)}
          className={`block w-full text-left text-sm rounded px-2 py-1 ${
            isCurrent
              ? "bg-[var(--color-surface-hover)] text-[var(--color-accent)] font-semibold cursor-default"
              : "text-[var(--color-text)] hover:bg-[var(--color-surface-hover)]"
          }`}
        >
          <span className="mr-1.5 text-xs text-[var(--color-accent)]">{node.session.project_label}</span>
          <span className="truncate">{node.session.title}</span>
          {isCurrent && <span className="ml-2 text-xs">● you</span>}
        </button>
        {node.children.map((c) => renderNode(c, depth + 1))}
      </div>
    );
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-full max-w-2xl max-h-[80vh] overflow-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-2 flex items-center justify-between">
          <span className="text-sm font-medium text-[var(--color-text)]">Session lineage</span>
          <button
            onClick={onClose}
            className="text-sm text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
          >
            ×
          </button>
        </div>
        {renderNode(root, 0)}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Typecheck**

Run: `cd crates/webui/frontend && npx tsc --noEmit`
Expected: clean (component unused until Task 4).

- [ ] **Step 3: Commit**

```bash
git add crates/webui/frontend/src/components/LineageTree.tsx
git commit -m "feat(webui): LineageTree overlay (indented family tree, click-to-open)"
```

---

### Task 4: Frontend — wire triggers (App + Transcript + Sidebar)

**Files:**
- Modify: `crates/webui/frontend/src/App.tsx`
- Modify: `crates/webui/frontend/src/components/Transcript.tsx`
- Modify: `crates/webui/frontend/src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: `LineageTree`. New prop `onViewTree` on Transcript (`() => void` | null) and Sidebar (`(s) => void`).

- [ ] **Step 1: Transcript — add the `⑂ Tree` header button**

In `Transcript.tsx` `Props`, add:

```tsx
  /** Open the lineage tree for this session; omitted/null when it has no lineage. */
  onViewTree?: (() => void) | null;
```

Destructure it: `}: Props) {` → add `onViewTree,` to the param list (after `visible = true`). Then in the header's `<div className="ml-auto flex gap-2">`, add as the first child:

```tsx
          {onViewTree && (
            <button
              onClick={onViewTree}
              className="text-xs px-2 py-1 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors"
              title="View this session's fork/copy lineage"
            >
              ⑂ Tree
            </button>
          )}
```

- [ ] **Step 2: Sidebar — make the `⑂`/`⧉` badges open the tree**

In `Sidebar.tsx` `Props`, add `onViewTree: (s: SessionSummary) => void;` and destructure `onViewTree,`. Replace the two badge `<span>`s (the `⑂` and `⧉` count badges) with clickable versions:

```tsx
                    {childrenBySource.has(s.id) && (
                      <span
                        onClick={(e) => {
                          e.stopPropagation();
                          onViewTree(s);
                        }}
                        className="text-[var(--color-accent)] opacity-80 cursor-pointer hover:opacity-100"
                        title={`${childrenBySource.get(s.id)!.length} session(s) forked from this — view tree`}
                      >
                        ⑂ {childrenBySource.get(s.id)!.length}
                      </span>
                    )}
                    {copiedChildrenBySource.has(s.id) && (
                      <span
                        onClick={(e) => {
                          e.stopPropagation();
                          onViewTree(s);
                        }}
                        className="text-[var(--color-accent)] opacity-80 cursor-pointer hover:opacity-100"
                        title={`${copiedChildrenBySource.get(s.id)!.length} session(s) copied from this — view tree`}
                      >
                        ⧉ {copiedChildrenBySource.get(s.id)!.length}
                      </span>
                    )}
```

- [ ] **Step 3: App — state, overlay, and wiring**

In `App.tsx`, add the import:

```tsx
import LineageTree from "./components/LineageTree";
```

Add state (near `target`):

```tsx
  const [lineageFor, setLineageFor] = useState<string | null>(null);
```

Pass `onViewTree` to `<Sidebar … />` (alongside the other props):

```tsx
              onViewTree={(s) => setLineageFor(s.id)}
```

In the Manager IIFE, after `const copies = sessions.filter(...)`, compute lineage presence:

```tsx
            const hasLineage =
              !!(openSummary && (openSummary.forked_from || openSummary.copied_from)) ||
              sessions.some((s) => s.forked_from === transcript.id || s.copied_from === transcript.id);
```

Pass it to `<Transcript … />`:

```tsx
                    onViewTree={hasLineage ? () => setLineageFor(transcript.id) : null}
```

Render the overlay — just before the final closing `</div>` of the top-level `return` (i.e. after `</main>`), add:

```tsx
        {lineageFor && (
          <LineageTree
            sessions={sessions}
            centerId={lineageFor}
            onOpen={(s) => {
              openSession(s);
              setLineageFor(null);
            }}
            onClose={() => setLineageFor(null)}
          />
        )}
```

- [ ] **Step 4: Full verification**

Run: `cd crates/webui/frontend && npx tsc --noEmit && npx vitest run && npx vite build`
Expected: tsc clean; all tests pass; build succeeds.

- [ ] **Step 5: Commit**

```bash
git add crates/webui/frontend/src/App.tsx crates/webui/frontend/src/components/Transcript.tsx crates/webui/frontend/src/components/Sidebar.tsx
git commit -m "feat(webui): lineage tree triggers — transcript button + sidebar badges"
```

---

## Self-Review

**Spec coverage:**
- `message_snippets` + `resolve_branch_messages` → Task 1. ✓
- `buildLineageTree` (walk-to-root, child grouping, edge classify, cycle guard) → Task 2. ✓
- Overlay (indented tree, snippet resolve, current-node mark, click-to-open, Esc/backdrop close) → Task 3. ✓
- Triggers (transcript header button gated on lineage; sidebar badges; App state) → Task 4. ✓
- Tests: `message_snippets` (Task 1), `buildLineageTree` (Task 2). ✓

**Placeholder scan:** none — complete code each step.

**Type consistency:** `LineageNode { session, edge, branchMessageUuid, branchParentFilePath, copiedOverlap, children }` matches the builder, the overlay's `walk`/`renderNode`, and the `resolve_branch_messages` payload (`{ file_path, message_uuid }` in, `{ message_uuid, snippet }` out). `onViewTree` is `(() => void) | null` on Transcript, `(s) => void` on Sidebar — matching App's two call sites. `centerId`/`onOpen`/`onClose` match the LineageTree props and App's render.
