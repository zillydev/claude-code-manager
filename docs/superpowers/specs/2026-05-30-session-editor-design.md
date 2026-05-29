# claude-manager — Session History Editor

**Date:** 2026-05-30
**Status:** Draft — iterating on feature surface
**Goal:** Turn the webui from a read-only transcript viewer into a full-featured editor for Claude Code session history — manual editing, structural edits, reordering, and moving/copying messages and sessions across sessions and projects.

> **Phase 0 result (2026-05-30): PASS.** A fully-regenerated, flattened, copy-on-edit session file (new session UUID, regenerated `parentUuid` chain, lossless raw-value rewrite via `core::session::edit::commit`) was resumed with `claude --resume`. Claude Code accepted the file, reconstructed conversation context from it (it correctly recalled the prior user message), and continued appending new turns to it normally (file grew from 5 → 20 lines, all appends carrying the resumed `sessionId`). No special envelope fields beyond what `commit` already writes were required. The riskiest assumption in this design is validated; UI work (phases 1+) is unblocked.

---

## 1. Motivation

Claude Code reconstructs conversation context entirely from the session `.jsonl` on resume. We verified experimentally that editing user messages, assistant messages, and even `<synthetic>` system turns in the transcript all flow into the model's context on the next turn. That makes the transcript a **fully editable substrate** for steering Claude: prune dead ends, rewrite a bad answer, graft useful exchanges from another session, reorder for clarity, then resume.

This spec designs that editor to feel like a real text editor — undo/redo, cut/copy/paste, find/replace, multi-select, drag-and-drop — but operating on conversation turns instead of lines of text.

---

## 2. Settled decisions (from brainstorming)

| Decision | Choice | Rationale |
|---|---|---|
| **Safety model** | **Copy-on-edit** | Every commit writes a NEW session file (new UUID). Originals are never mutated. Zero risk to live sessions; original always recoverable. |
| **End goal** | **Edit only** | The tool produces valid edited session files; the user runs `claude --resume <id>` themselves. (Resume command is surfaced for convenience.) |
| **Invariants** | **Guard-railed core** | The UI cannot produce a broken file. Core re-stitches `parentUuid`, regenerates UUIDs, keeps `tool_use`↔`tool_result` paired, rebuilds leaf/`lastPrompt`. |
| **Tree on commit** | **Flatten to active path** | New session is a clean linear chain of the rendered turns. Abandoned rewind-branches dropped. |
| **Edit unit** | **Both granularities** | Turn-level by default (a user msg, or an assistant response + its tool calls/results as one bundle); expand a turn to edit individual blocks. |
| **Interaction** | **Drag-and-drop + context menu** | Drag to reorder/move; right-click for copy/move/delete; selection for bulk ops. |
| **Branch model** | **Fork to new session** | A branch is just a new flattened session file. It appears in the session list automatically — no in-file tree, no separate tree view. Reuses the commit pipeline (a fork = commit of the prefix `root..N`). |
| **Edit vs branch** | **Replace by default** | Edits replace in place; branching is a separate explicit action ("Fork from here"). |

---

## 3. Core invariants (what "valid" means)

Every committed file must satisfy these, enforced by the core write pipeline:

1. **Linear parent chain.** Events on the path form a single chain: each event's `parentUuid` = the previous event's `uuid`; the first event's `parentUuid` = `null`.
2. **Unique event UUIDs.** Regenerated on every commit so copies/moves never collide.
3. **Tool pairing.** Every assistant `tool_use` block has exactly one following user `tool_result` with a matching `tool_use_id`, and every `tool_result` has a preceding `tool_use`. Orphans are auto-repaired (synthesize a placeholder result for a dangling `tool_use`; drop a dangling `tool_result`).
4. **Tool-result adjacency.** A `tool_result` user event must immediately follow the assistant event carrying its `tool_use` (the API requires this). Turn-level ops move the pair as a unit.
5. **Consistent session identity.** Every event carries the same (new) `sessionId`, and the `cwd`/`gitBranch` reflect the target project.
6. **Resolvable leaf.** A trailing `last-prompt` bookmark points at the final event; the title-resolution chain (custom-title > lastPrompt > last user message) still works.
7. **Role/shape validity.** `user` and `assistant` roles only; content blocks well-formed per `event.rs`.

A **validation pass** runs before every write and the UI surfaces any auto-repairs it performed.

---

## 4. Feature surface (the full vision)

### 4.1 In-session content editing
- **Edit message text** — inline markdown editor with edit/preview toggle; code-block + JSON syntax highlighting.
- **Edit any block** — text, thinking, `tool_use` input (JSON editor), `tool_result` content, `is_error` flag.
- **Edit turn metadata** — role (user↔assistant), model label, timestamp.
- **Raw mode** — drop to raw JSON for a single event with live validation (advanced escape hatch).

### 4.2 Structural editing (within a session)
- **Delete** turn(s) or block(s).
- **Insert** a new turn (blank user or assistant) at any position; insert a block within a turn.
- **Duplicate** a turn or block.
- **Split** a turn into two; **merge** adjacent same-role turns.
- **Reorder** — drag turns; move up/down; drag a block within or between turns.
- **Bulk ops** — multi-select (shift-range, cmd-toggle) then delete/move/copy/reorder together.

### 4.3 Text-editor-grade affordances
- **Undo / redo** — multi-level, every op is reversible (in-memory edit stack before commit).
- **Cut / copy / paste** — a turn clipboard; paste into the same or another open session.
- **Find & replace** — within the session; optional scope to all sessions (uses the existing transcript search). Regex toggle.
- **Diff preview** — original vs edited, side-by-side, before committing.
- **Keyboard shortcuts** for every op; discoverable via a shortcut cheat-sheet.
- **Dirty-state tracking** — unsaved-changes indicator; warn on navigate-away; explicit Commit / Discard.

### 4.4 Cross-session operations
- **Move / copy turns between sessions** — open 2+ sessions (tabs or split pane); drag turns A→B or use Copy/Move-to picker. Copy-on-edit means both endpoints become new files (source minus turns, target plus turns); originals untouched.
- **Merge sessions** — concatenate session B's active path onto A.
- **Split a session** — fork into two new sessions at a chosen turn.
- **Clone session** — straight copy with a new UUID.
- **New blank session** — create an empty session in a project.

### 4.5 Cross-project / directory operations
- **Move / copy a session to another project** — file-level: rewrite `cwd`/`gitBranch`/dir encoding; copy = new UUID, move = relocate (keep UUID) + remove original.
- **Project picker** — choose any existing project dir, or type a new cwd path.

### 4.6 Session lifecycle management
- **Rename** (writes/updates `custom-title`).
- **Delete session** → trash with restore (never hard-delete originals silently).
- **Duplicate** session.
- **Export** to Markdown or JSON; **Import** a Markdown/JSON file as a new session.

### 4.7 Safety & lifecycle
- **Copy-on-edit** — commits always produce new files.
- **Trash + restore** — deletes and "move" sources go to a `~/.claude/.trash` (or app-managed) area with a restore action.
- **Live-session detection** — cross-reference `~/.claude/sessions/<pid>.json`; badge live sessions and warn before editing-from one (editing is still safe since we copy).
- **Resume command surfacing** — after commit, show copyable `claude --resume <new-id>` (and the new file path / project).
- **Validation panel** — lists invariant repairs applied on the last commit.

### 4.8 Branching

A branch is just a **new forked session** produced by the commit pipeline — it appears in the session list automatically, is independently resumable, and needs no special tree storage or visualization. All branching ops below are thin wrappers over `commit(prefix_or_edited_events, …)`.

**Fork operations:**
- **Fork from latest** — clone the whole active path into a new session. (Same as Clone; the entry point is "branch from where I am now.")
- **Fork from a message (mid-session)** — new session = the prefix `root..N` (inclusive). Everything after the chosen message is dropped. Resume the fork and it continues *from that point* in a new direction. This is the headline feature ("branch from a particular message in between").
- **Fork before a message** — prefix `root..N-1`, i.e. rewind to just before message N (drop N and everything after). Convenient for "redo from here without the last exchange."
- **Fork-and-edit** — chains fork + open-for-edit: fork at N, then immediately land in the edited message so you can change it and continue. The common "rewind, tweak my prompt, try again" loop in one action.
- **Multiple forks from one point** — fork the same message repeatedly to explore N alternatives; each is its own session, all visible in the list. (A small "fork count" affordance can pre-seed distinct titles.)

**Fork provenance (lightweight):**
- Each fork records optional metadata in the new file: `forkedFrom: { sessionId, leafUuid, forkPointIndex }`. Purely informational — used to show a "forked from *<title>* @ message N" line and to enable an optional **"show forks of this session"** / **"jump to parent"** filter in the list. Does not affect resume. (If we skip this, forks still work; they're just not grouped.)

**Fork titling:**
- Auto-title a fork as the prefix's last user message (existing title logic already does this), or let the user set a `custom-title` at fork time (e.g. "alt: skip the refactor").

**Selecting the fork point:**
- In the transcript, every turn gets a **"Fork from here"** action (context menu / hover button). The fork point is a turn on the active path; core maps it to the underlying event index and commits `root..index`.

**Explicitly out of scope (v1):** surfacing *pre-existing* in-file branches that Claude Code's own rewinds created (the abandoned sibling subtrees). We continue to operate on the active path only. Exposing those is a possible future "tree view" enhancement (the rejected "in-file tree / Both" option) and is noted in §7.

---

## 5. Architecture

```
ui (React)
  editor state: EditableSession + edit-op stack (undo/redo) + clipboard + selection
  components: TurnEditor, BlockEditor, DiffView, MovePicker, ValidationPanel, Tabs/SplitPane
        │  invoke / events
        ▼
webui (Tauri commands)  ── thin; delegates to core::edit
        │
        ▼
core::edit (NEW)        ── the only writer of session files
  EditableSession  · EditOp · apply() · validate() · commit()
core::session, core::store (existing read path, reused)
```

### 5.1 `core::session::edit` module
- **`EditableSession`** — `{ source_path, project_label, events: Vec<Event> }` where `events` is the resolved active path. Plus a `turns()` view that groups events into turns with their event-index ranges (reusing render-grouping logic) so the UI gets both granularities.
- **`EditOp`** (serializable enum, sent from the UI) — `EditText`, `DeleteTurn`, `DeleteBlock`, `InsertTurn`, `InsertBlock`, `ReorderTurn`, `MoveBlock`, `SplitTurn`, `MergeTurns`, `SetRole`, `SetModel`, `SetTimestamp`, `PasteTurns`, … Each op has an inverse for undo (undo/redo can also be implemented purely UI-side by re-sending the full desired state — see §7).
- **`commit(events, target_project, mode) -> CommitResult`** — runs the §3 write pipeline; returns `{ new_session_id, new_path, repairs: Vec<Repair> }`.
- **`validate(events) -> Vec<Violation>`** — pure check used by the UI before commit and by tests.

### 5.2 Serialization for editing
The current `RenderedSession` is lossy (merges turns, drops uuids). The editor needs an **`EditableSession` DTO** that exposes raw events with stable IDs, tool pairing made explicit, and the turn grouping — so the frontend can address, reorder, and round-trip events back to core.

### 5.3 New Tauri commands
- `open_for_edit(file_path, project_label) -> EditableSession`
- `validate_events(events) -> Vec<Violation>`
- `commit_session(events, target_project_path, opts) -> CommitResult`  *(in-session edits, reorder, merge/split-as-new, paste-into-target)*
- `list_projects() -> Vec<ProjectInfo>`  *(move/copy targets; distinct cwds + dir names)*
- `move_or_copy_session(source_path, target_project_path, mode) -> CommitResult`  *(file-level)*
- `fork_session(source_path, fork_point_index, opts) -> CommitResult`  *(commit prefix `root..fork_point_index`; opts: inclusive/exclusive, custom_title, record provenance)*
- `trash_session(file_path)` / `restore_session(trash_id)` / `list_trash()`
- `export_session(file_path, format)` / `import_session(path_or_content, target_project, format)`

---

## 6. Phasing

Each phase is independently shippable and de-risks the next.

- **Phase 0 — Spike (de-risk the core claim).** Generate one flattened, fully-regenerated session file and confirm `claude --resume <id>` accepts it and reconstructs context. Pin down the minimal required envelope fields. *Gate for everything else.*
- **Phase 1 — Single-message edit + commit pipeline.** `core::edit` skeleton, `EditableSession` DTO, edit-text op, `validate`, `commit` (copy-on-edit, flatten), resume-command surfacing, diff preview, dirty-state. Proves the write path end to end.
- **Phase 1.5 — Branching/fork.** "Fork from here" + "fork from latest" + fork-before-N, riding directly on the Phase 1 commit pipeline (fork = commit of a prefix). Optional fork provenance + "show forks" filter. Cheap to add once Phase 1 lands; high value for the steering use case.
- **Phase 2 — In-session structural edits.** Delete / insert / duplicate / reorder (drag-and-drop) / split / merge / role+metadata edit / block-level editing / undo-redo / multi-select / bulk ops. (Fork-and-edit chains Phase 1.5 fork + this phase's edit.)
- **Phase 3 — Editor affordances.** Cut/copy/paste turn clipboard, find & replace, keyboard shortcuts + cheat-sheet, raw-JSON mode.
- **Phase 4 — Cross-session.** Tabs / split-pane, move/copy turns A→B, merge sessions, split into new sessions, clone, new blank session.
- **Phase 5 — Cross-project + lifecycle.** Move/copy session to project, project picker / new cwd, rename, trash+restore, export/import, live-session detection.

---

## 7. Open questions (to resolve while iterating)

1. **Undo/redo model.** Pure client-side full-state snapshots (simplest, since commit is what touches disk) vs. invertible `EditOp`s in core. Leaning: client-side snapshot stack; core stays stateless.
2. **Where does the trash live?** `~/.claude/.trash/claude-manager/` vs. an app data dir. Restore needs original path + project.
3. **New-file proliferation on cross-session move.** Copy-on-edit means moving a turn A→B spawns edited copies of both A and B. Acceptable, or do we want an opt-in "edit original in place" for power users on non-live sessions?
4. **Manual/inserted turns** — do synthesized assistant turns need a model label and plausible token usage, or is empty fine for resume? (Phase 0 informs this.)
5. **Timestamps** — auto-assign monotonic timestamps on insert/reorder, or preserve originals (which may then be out of order)?
6. **Sidecar data** on session move (subagents/, file-history/, tasks/) — relocate, or drop? Likely drop (flattened session doesn't reference them).
7. **Markdown/JSON export round-trip fidelity** — is import lossless enough to be useful, or export-only initially?
8. **Multi-session editing UI** — tabs vs. split-pane vs. both, and how the clipboard spans them.
9. **Fork provenance** — worth recording `forkedFrom` + a "show forks of this session" filter, or keep forks as plain unrelated sessions? (Cheap to add; aids discoverability when you fan out many alternatives.)
10. **Surfacing pre-existing in-file branches** — sessions that the user rewound inside Claude Code already contain abandoned sibling branches we currently ignore. Future "tree view" could let you fork from those too. Out of scope for v1; revisit if needed.

---

## 7b. Known limitations (foundation implementation, 2026-05-30)

Shipped in the foundation (edit-text + fork). Both are non-blocking for the normal open→edit/fork flow and bounded by copy-on-edit (originals never mutated):

1. **Live-tail index collision.** Edit/fork indices (`edit_index`/`fork_index`) are computed by `render_session` over the full active path. But the live-tail path (`tail_loop` in `webui/commands.rs`) renders only the newly-appended event *fragment*, so those turns get indices recomputed from 0 that collide with early turns. Editing/forking a turn that arrived via live-tail *while the session is open* would target the wrong raw line. Reopening the session (`open_session`) re-renders with correct absolute indices. **Fix (follow-up):** pass a base message-offset into the tail render, or re-derive indices for appended turns; touches `tail_loop` + the live-tail handler in `useSessions.ts`.
2. **Latent parser-acceptance gap.** The raw index space (`read_raw`, keeps any valid-JSON line) and the typed index space (`parse_file`, requires a `message` field on user/assistant) coincide for all 214 real sessions today, but a hypothetical message-less `user`/`assistant` line on the active path would desync them (typed parser drops it; raw keeps it), mis-targeting an edit/fork in the *new* file. No such line exists in practice. **Fix (if ever needed):** index over a single source of truth (raw) end to end.
3. **Fork at a tool-ending assistant turn drops the real tool result.** `fork_index` is the last *assistant* event of a merged turn; the fork cutoff stops there, so a pure `tool_result` user line that immediately follows is excluded. The commit pipeline then repairs the now-dangling `tool_use` with a `"[result removed during editing]"` placeholder. The forked file is valid and resumable and the original is untouched, but the real tool output is lost and the placeholder wording is misleading for a fork. In practice fork points are usually user turns or text answers, so this is rare. **Fix (follow-up):** extend the fork cutoff to include trailing pure-`tool_result` user lines of the final merged assistant event; give the repair a fork-specific message.

## 8. Testing strategy

- **Phase 0 acceptance:** real `claude --resume` against a generated file (manual, documented).
- **Unit (core):** `validate` catches each invariant violation; `commit` produces a chain that re-parses and passes `validate`; tool-pairing repair cases; flatten drops branches; cross-project cwd rewrite.
- **Property/round-trip:** load → no-op commit → reload yields an equivalent active path.
- **Snapshot:** `EditableSession` DTO for a fixture session.
- **Frontend:** edit-op application + undo/redo on a fixture; drag-reorder produces expected order.
- **Manual smoke:** end-to-end edit → commit → resume for each phase's headline op.
