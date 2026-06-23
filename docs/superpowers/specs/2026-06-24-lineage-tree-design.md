# Lineage Tree Overlay Design

**Date:** 2026-06-24
**Status:** Approved
**TODOS.md item:** UI overhaul → "brainstorm on how best to show forked sessions … some tree UI for … where a session began, and how many different sessions were forked from it"

## Goal

Turn the existing *flat* fork/copy lineage (sidebar badges + a copy banner) into a
**visual tree** of a session's whole family — the root it began from and every
fork/copy that branched off, recursively — opened **on demand as an overlay**,
rendered as an **indented tree**, with click-to-jump navigation.

## Decisions (from brainstorming)

- **Scope:** one session's family (not a global explorer).
- **Placement:** on-demand overlay/modal (not inline/side-panel/tab).
- **Layout:** indented tree (file-explorer style), not git-graph or horizontal.

## Data (already present)

Each `SessionSummary` carries: `forked_from` (explicit CC fork parent),
`copied_from` (inferred shared-uuid ancestor), `forked_from_message` /
`copied_from_message` (the parent message uuid it branched at), `copied_overlap`.
A session has **≤1 parent** (`forked_from` *or* `copied_from`) and any number of
children, so the family is a tree. Lineage ids are global, so a family can span
projects. The frontend already holds the full `sessions` list, so the tree
**structure** needs no backend.

The one thing not in `sessions` is the **branch-message text** (it lives in the
parent session's `.jsonl`), so that's resolved by a small backend command.

## Backend

### `core` — message snippet resolution

`crates/core/src/store/mod.rs` (or a small sibling): add

```rust
/// Short single-line previews for specific message uuids in one session file.
/// Parses the file once; returns only the requested uuids that resolve to text.
pub fn message_snippets(path: &Path, wanted: &HashSet<Uuid>) -> HashMap<Uuid, String>
```

It parses via `parse_file`, walks events, and for each event whose `uuid()` is in
`wanted` extracts a preview: a `User` turn's `MessageContent::Text`, or an
`Assistant` turn's first `Block::Text` — stripped to one line and truncated
(~60 chars). Non-text / unresolved uuids are simply omitted.

### `webui` — `resolve_branch_messages` command

`crates/webui/src/commands.rs`:

```rust
#[derive(Deserialize)] pub struct BranchRef { pub file_path: String, pub message_uuid: String }
#[derive(Serialize)]   pub struct BranchSnippet { pub message_uuid: String, pub snippet: String }

#[tauri::command]
pub async fn resolve_branch_messages(refs: Vec<BranchRef>) -> Result<Vec<BranchSnippet>, String>
```

Groups `refs` by `file_path`, calls `message_snippets` once per file (so each
parent is parsed at most once), and returns the flattened `{message_uuid, snippet}`
list. Runs on the blocking pool (it parses files). Registered in `lib.rs`.

## Frontend

### `lib/lineageTree.ts` — pure tree builder

```ts
export type LineageEdge = "root" | "forked" | "copied";
export interface LineageNode {
  session: SessionSummary;
  edge: LineageEdge;
  branchMessageUuid: string | null;   // forked_from_message / copied_from_message
  branchParentFilePath: string | null; // parent's file_path (where the snippet lives)
  copiedOverlap: number | null;
  children: LineageNode[];
}
export function buildLineageTree(
  sessions: SessionSummary[],
  centerId: string,
): LineageNode | null;
```

- Map `id → session`. From `centerId`, walk up `forked_from ?? copied_from` to the
  **root** (stop when the parent isn't in the map; **cycle-guard** with a visited set).
- From the root, recurse: a node's children are sessions whose
  `(forked_from ?? copied_from) === node.id`. Per child, `edge = forked_from === parent.id
  ? "forked" : "copied"`; `branchMessageUuid = forked_from_message ?? copied_from_message`;
  `branchParentFilePath = parent.session.file_path`. Cycle-guard the descent too.
- Returns the root node (the whole family), or `null` if `centerId` isn't found.
  A session with no parent and no children yields a single-node tree (caller decides
  whether that's worth showing — the trigger is gated on having lineage).

### `components/LineageTree.tsx` — overlay + renderer

Props: `{ sessions, centerId, onOpen(s), onClose() }`. On mount it
`buildLineageTree`, collects every node's `{branchParentFilePath, branchMessageUuid}`
pair, calls `resolve_branch_messages`, and renders.

- **Overlay:** a centered modal over a dimmed backdrop. **Esc** and **click-outside**
  close. Scrolls if tall.
- **Indented rows** with connector lines. Each row: an edge glyph + label
  (`◀ origin` for root; `⑂ forked at "<snippet>"`; `⧉ copied · N shared`), then the
  session title + project label. The **`centerId` node is highlighted and marked
  "● you"** and is not clickable; every other node is a button → `onOpen(session)`
  (which closes the overlay and opens that session via the existing path).
- Fork vs copy reuse the `⑂` / `⧉` glyphs + accent palette already used in the sidebar.

### Triggers

- **Transcript header:** a `⑂ Tree` button (next to Compact/Details), shown only
  when the open session **has lineage** (a parent, or any session forked/copied from
  it). Opens the overlay centered on the open session.
- **Sidebar badges:** the existing `⑂ N` / `⧉ N` count badges become clickable →
  open the overlay centered on that row's session.
- **App state:** `lineageFor: string | null` (a session id). A trigger sets it;
  `lineageFor && <LineageTree centerId={lineageFor} … />` renders the overlay;
  `onOpen`/`onClose` clear it.

## Testing

- **vitest** on `buildLineageTree`: root-finding from a deep child, child grouping,
  forked-vs-copied edge classification, current-node identity, and a cycle guard
  (a self/loop reference must not hang).
- **core test** for `message_snippets`: a file with known uuids → correct snippets;
  unknown uuid omitted; long text truncated.
- Overlay rendering + triggers verified in-app.
- `cargo test`, `tsc --noEmit`, `vitest run`, `vite build` all green.

## Edge cases

- **No lineage:** triggers don't appear (header button gated; no badges) → overlay
  never opens for a lone session.
- **Parent not discovered** (e.g. trashed): walk-up stops there; that node becomes
  the visible root. Snippet for its branch is just omitted ("forked here").
- **Cross-project family:** included — `sessions` spans all projects.
- **Unresolvable snippet:** node shows "forked here" / "copied" without text.

## Non-goals

- No global fork explorer (deferred — separate cycle).
- No git-graph / horizontal layouts.
- No editing/forking from the tree (it's a read-only navigator).
- No new lineage *computation* — reuses the existing `forked_from`/`copied_from` data.
