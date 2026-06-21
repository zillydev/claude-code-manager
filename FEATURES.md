# claude-manager — Features

A tool to browse, search, edit, compact, harvest, and resume Claude Code session
`.jsonl` files. A **Tauri 2 + React 19 webui** and a **ratatui TUI** over a shared
Rust core (`claude-manager-core`).

> **Maintenance:** update this file as the last step of every feature merge. Each
> entry links to its design spec in `docs/superpowers/specs/` (and plan in
> `docs/superpowers/plans/`) for the full rationale. Per-feature specs are the
> source of truth for *why/how*; this file is the index of *what exists now*.

## Architecture

- **Cargo workspace:** `crates/core` (lib — parsing/render/store/surgery),
  `crates/tui` (ratatui binary `claude-manager`, also hosts the `surgery` CLI),
  `crates/webui` (Tauri 2 + React/TS/Vite/Tailwind).
- Operates on `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl` — one JSON object
  per line, linked into a tree by `parentUuid`.
- **Copy-on-edit safety:** every fork/edit/delete/surgery writes a NEW session
  file; originals are never mutated (sole exception: rename, which appends a
  `custom-title` line in place).
- Run: `make tui` · `make webui` (`cargo tauri dev --no-watch`) · `make build` ·
  `make test`.

## Browsing & viewing (webui + TUI)

- Sessions list — project, title, tokens, updated; project/directory/time
  filters; sort; sidebar resize/collapse. → `2026-05-23-claude-manager-tui-design`,
  `2026-05-24-webui-design`
- Transcript rendering — consecutive assistant turns merged, markdown via
  `@tailwindcss/typography`, tool calls with full input on expand, synthetic
  turns skipped.
- Session info panel — token cost, per-turn model, git branch, cwd.
- **Compact** toggle (user turns + adjacent assistant) and **Details** toggle —
  both persisted across restarts.
- **Pre-compaction history** — renders all conversation trees in a compacted
  file with `── segment k/N · pre-compaction|active ──` dividers; pre-compaction
  turns are view-only (only the active tree is editable). → `2026-06-06-pre-compaction-history-design`

## Search

- **In-session find** (⌘F) — case / word / regex toggles, highlight-all + active
  match via the CSS Custom Highlight API, next/prev wrap, editable
  jump-to-match counter. Memoized (`React.memo`) so large sessions don't lag. →
  `2026-06-05-in-session-find-design`
- Cross-transcript search by content (sidebar search box).

## Editing & surgery

- **Edit a message / fork from here** — copy-on-edit → a new resumable session;
  resolves targets by stable uuid (incl. pre-compaction turns). → `2026-05-30-session-editor-design`
- **Delete a session** (trash with restore) / **delete a message**.
- **Rename a session** — in-place additive `custom-title`; titlebar + sidebar
  update live. → `2026-06-12-rename-session-design`
- **`session-surgery` skill + CLI** (`surgery extract` / `rewrite`) — summarize
  or prune a turn-snapped message range → a new resumable session (timestamps
  backfilled so `claude --resume` accepts it). `--from/--to` accept a uuid
  **prefix**. → `2026-05-30-session-surgery-skill-design`, `2026-06-18-surgery-rewrite-parity-design`

## Lineage

- **Linked / forked sessions** — sidebar badges (`⮌ from …`, `⑂ N`) + a
  transcript banner, from CC's `forkedFrom`. → `2026-06-14-linked-sessions-design`
- **Cross-session copy lineage** — inferred from shared message-uuid overlap
  (handles background-restart / manual-`/compact` copies that split a session
  across files); bidirectional badges + banner. → `2026-06-17-compaction-lineage-design`
- **Inline "forked from here" markers** — at the deepest copied message (the
  real branch point) in the parent session. → `2026-06-17-fork-point-markers-design`

## Harvest (read-only session mining)

- **`session-harvest` skill + `surgery extract --outline [--all-segments]`** —
  outline-first navigation (compact one-line-per-turn map with 8-char short ids
  + salient tool args, `· compaction summary` tags), drill down by uuid-prefix
  range, classify spans → produce a **skill / kb entry / script / summary**.
  Ranks candidates by reuse over novelty; checks for an already-written
  distillation before proposing kb/summary. Read-only re sessions. →
  `2026-06-18-session-harvest-design`, `-harvest-all-segments-`, `-sharpen-harvest-tooling-`
- Outline & drill-down clean machinery: command echoes → `/cmd`,
  task-notifications collapsed, slash-command/empty turns skipped, embedded skill
  prompts elided. → `2026-06-18-clean-noisy-messages-design`, `-elide-skill-prompts-design`

## Multi-window

- **Multiple windows** (⌘N + native menu) — one per macOS Space; per-window
  live-tail isolation (watcher + events scoped by window label). → `2026-06-14-multi-window-design`

## Embedded terminal

- **Manager | Terminal tabs** in the session pane (sidebar always visible). The
  Terminal tab runs the real interactive `claude --resume <open session>` in a
  PTY (xterm.js + `portable-pty`) — full TUI, exactly like Ghostty. One
  persistent terminal per session, **spawned lazily** only when a session's
  terminal first becomes visible; uses the user's bypass alias (`claude-tracked
  --dangerously-skip-permissions`). Debounced window resize. Cleanup on window
  close + a `RunEvent::Exit` backstop. **⌘E** toggles tabs. → `2026-06-19-terminal-tab-design`

## Live tail & scrolling

- Open sessions **live-tail** their file — new turns stream in (so a session
  being written, incl. by the embedded terminal, updates in real time).
- **Tail-follow** — auto-scroll to a new turn only when the user is pinned to the
  bottom; a scrolled-up reader is left alone. Scroll-to-bottom restored when
  returning from the Terminal tab.

## Conventions & dev notes

- Workflow per feature: **brainstorm → spec → plan → execute → finish →
  user-directed merge** (`--no-ff`, tests green, delete branch).
- **Never launch the webui from automation** — the user keeps it open across
  Spaces; `make webui` uses `--no-watch` (Rust changes need a manual restart;
  frontend hot-reloads via Vite).
- `~/.claude/settings.json` has `cleanupPeriodDays: 36500` so sessions aren't
  auto-pruned.
- Skills live in `skills/` (`session-surgery`, `session-harvest`).

## Deferred / parked

- **`feat/chat-webui`** (branch, not on `main`) — in-webui agentic chat by
  driving headless `claude`; parked (custom chat UI needed too much refinement)
  and **superseded by the embedded terminal tab**. Kept as a fallback.
- Other deferred items: retroactive fork-link backfill (proven unsafe);
  live-session detection on edit/delete; session-editor later phases.
