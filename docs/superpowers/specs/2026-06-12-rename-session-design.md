# Rename a session — design

Date: 2026-06-12
Status: Approved

## Goal

Let the user rename a session from the claude-manager sidebar. A rename is a
**real** Claude Code rename: it appends a `custom-title` event to the session's
`.jsonl`, so the new name shows in both claude-manager and `claude --resume`.

## Persistence (the one architectural decision)

Append `{"type":"custom-title","customTitle":"<title>","sessionId":"<id>"}` to the
session file — exactly what Claude Code's `/rename` writes. The recent
last-custom-title-wins fix means the appended title becomes the displayed title.

This deliberately **mutates the original file**, the one thing copy-on-edit
otherwise avoids. It's acceptable because a custom-title is pure metadata (never
conversation content), CC itself appends these in-place, and "rename" must keep the
same session id (a copy would not be a rename). Chosen over a claude-manager-only
sidecar so the rename is the real session title, visible in `/resume`.

custom-title events carry no timestamp (verified on real files); they are metadata,
not message lines, so Claude Code's discovery timestamp rule does not apply.

## Backend

- `crates/core/src/session/edit.rs`: `pub fn append_custom_title(path: &Path, title: &str) -> Result<()>`
  - Trim `title`; `bail!` if empty.
  - Derive `sessionId` from the file stem (the session uuid).
  - Build the line with `serde_json` (safe escaping).
  - Ensure the file ends with a newline (read the last byte; write `\n` first if
    missing), then append the line + `\n` in append mode (no rewrite of the file).
- `crates/webui/src/commands.rs`: `#[tauri::command] rename_session(file_path, new_title)`
  → calls `append_custom_title`. Register in `crates/webui/src/lib.rs`.

## Frontend

- `Sidebar.tsx`: add `editingId: string | null` + `draftTitle` state (parallel to the
  existing `confirmingId` delete state). A ✎ button appears on row hover next to the ×
  delete button. Clicking it sets `editingId = s.id`, `draftTitle = s.title`. While a
  row is being edited, it renders an autofocused `<input>` (with the value) in place of
  the select-`<button>` (an input can't be nested inside that button), plus save/cancel.
  Enter saves; Esc cancels; the input also cancels on blur.
- `App.tsx`: pass `onRename(session, newTitle)` to Sidebar that
  `invoke("rename_session", { filePath, newTitle })` then `refresh()` — same shape as
  the existing `onDelete`. Empty/whitespace title is a no-op (don't call the command).

## Testing

- Core (TDD): `append_custom_title` appends a custom-title; reading the file back and
  running `summarize` returns the trimmed new title (covers append + last-wins pick).
  A second test: empty/whitespace title is rejected.
- UI (manual): hover a row → ✎ → type → Enter updates the sidebar title; persists across
  refresh; appears in `claude --resume`.

## Caveats (noted, not blockers)

- Empty title rejected.
- Live-session hazard: if a running `claude` process owns the session, it may
  re-persist from memory and clobber the appended line (same class as edit/delete).
- Each rename adds one line (Claude Code does the same).
