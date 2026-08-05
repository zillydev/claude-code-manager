# claude-code-manager — Features

A tool to browse, search, edit, compact, harvest, and resume Claude Code session `.jsonl` files. A **Tauri 2 + React 19 desktop app** over a reusable Rust core (`claude-code-manager-core`).

> **Maintenance:** update this file as the last step of every feature merge. Each entry links to its design spec in `docs/superpowers/specs/` (and plan in `docs/superpowers/plans/`) for the full rationale. Per-feature specs are the source of truth for *why/how*; this file is the index of *what exists now*.

## Architecture

- **Cargo workspace:** `crates/core` (lib — parsing/render/store/surgery; deliberately front-end agnostic so another UI can be added later), `crates/app` (Tauri 2 + React/TS/Vite/Tailwind — the interface), `crates/cli` (the `claude-code-manager` binary: `surgery` subcommands, driven by the skills).
- Operates on `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl` — one JSON object per line, linked into a tree by `parentUuid`.
- **Copy-on-edit safety:** every fork/edit/delete/surgery writes a NEW session file; originals are never mutated (sole exception: rename, which appends a `custom-title` line in place).
- Run: `make dev` (`cargo tauri dev --no-watch`) · `make build` · `make test` · `make print` (CLI smoke test).

## Browsing & viewing

- Sessions list — project, title, tokens, updated (time of the last real conversation turn — live activity → discovered last-event → file mtime only for a turnless session — so the shown time, the Updated sort, and the time filter all agree and a non-turn file write doesn't bump it); project + time filters (one project selector, grouped by name, expands to directories only on name collisions); sort; sidebar resize/collapse. → `2026-05-23-claude-code-manager-tui-design`, `2026-05-24-webui-design`, `2026-06-21-unified-project-selector-design`
- **Chat-style sides** — user messages (and slash commands) sit on the **right** in a bordered surface bubble (`max-w-75%`); assistant turns stay full-width on the left. Role is then legible from the shape of the page while scrolling fast, instead of only from a small coloured label. Editing a user message expands to full width.
- Transcript rendering — consecutive assistant turns merged, **GFM** markdown (react-markdown + `remark-gfm` + `@tailwindcss/typography`: tables, strikethrough, task-lists; inline code without decorative backticks), tool calls with full input on expand, synthetic turns skipped. Slash-command invocations (e.g. `/deploy-gateway`) render as a compact command line; the injected skill body and `<local-command-…>` machinery stay hidden. Explanatory-style `★ Insight` blocks render as a terminal-style callout (accent rules + markdown body) instead of collapsing into inline-code pills.
- **Jump to latest** — a ↓ button at the transcript's bottom-right, shown only while scrolled up. It re-pins tail-follow as well as scrolling, so live turns resume following.
- Session info panel — token cost, per-turn model, git branch, cwd.
- **Compact** toggle — every prompt plus the first and last turn of each reply, so the tool-churn in the middle collapses. Runs close at the next prompt, at a **task notification** (a user-role event that prompts a fresh reply — otherwise a background task finishing late welds two replies together and the answer that closed the turn is demoted to the middle and hidden; the `task · status · summary` line is kept too, so the reply after it has a visible cause), *and* at the end of the transcript, so the answer to the latest prompt is always shown (one reply spans many turns: an `attachment`/`system` line starts a new one). Plus a **Details** toggle — both persisted across restarts.
- **Pre-compaction history** — renders all conversation trees in a compacted file with `── segment k/N · pre-compaction|active ──` dividers; pre-compaction turns are view-only (only the active tree is editable). → `2026-06-06-pre-compaction-history-design`

## Search

- **In-session find** (⌘F) — case / word / regex toggles, highlight-all + active match via the CSS Custom Highlight API, next/prev wrap, editable jump-to-match counter. Memoized (`React.memo`) so large sessions don't lag. → `2026-06-05-in-session-find-design`
- Cross-transcript search by content (sidebar search box). The same box also matches a session's project and **both** its names — the local alias and claude's auto-title, since a renamed row shows both. (Sorting by Title uses the alias when set.)

## Organization

- **Session organization** — claude-code-manager-local metadata over sessions, in a separate `session-meta.json` (same dir/env as the config; claude's `.jsonl` is never touched). Global **tags** (a `name+color` registry; colored chips on rows; a per-session Tags… editor with autocomplete; a sidebar any-of **tag filter**, which a newly created session inherits — so it isn't instantly filtered out of the list you're looking at; a **Manage tags…** window to recolor/rename/delete), a **local alias** (a rename that is NOT claude's `custom-title` — so no live-terminal desync; shown as the row's primary title with claude's auto-name dim beneath), and a per-session **note** (`✎` on rows, full text under the details panel). A session derived by copy-on-edit (fork / edit / delete) inherits the source's tags — tags only, not the alias (which would duplicate a name) or note. Live + broadcast across windows; dangling tags self-heal on load. → `2026-07-06-session-organization-design`

## Bookmarks

- **Message bookmarks** — flag any user/assistant message from its ⋯ menu (metadata-only, so pre-compaction turns work too); stored as the turn's head uuid in `session-meta.json` (`bookmarks: [uuid]`), live across windows. An accent **⚑** marks bookmarked turns; a header `⚑ n` dropdown lists this session's bookmarks in transcript order (click → scroll + flash, auto-expanding compact view if the turn is hidden); a sidebar-header **⚑** opens the all-sessions browser — grouped by session, snippets + times via `message_previews` — where a click opens that session scrolled to the message. Derived sessions do NOT inherit bookmarks. → `2026-08-05-message-bookmarks-design`

## Editing & surgery

- **Fork from here** — copies the session file **as-is** up to the chosen message (file order), changing only what session identity requires: a new `sessionId` on every line, `forkedFrom` on the cut line, a fresh `last-prompt`, and — only if the copy would otherwise end on an unanswered `tool_use` — one synthesized `tool_result`. Nothing is flattened, trimmed, or re-uuid'd, so abandoned rewind branches and pre-compaction segments (including the `isCompactSummary` bridge) come along. The cut extends over the `tool_result` lines answering it, so a call is never separated from its result. Resolves the target by stable uuid, so the live-tail's batch-local indices can't truncate it. → `2026-05-30-session-editor-design`
- **Edit a message / delete a message** — copy-on-edit → a new flattened, resumable session (`edit::commit`: active path only, uuids regenerated and relinked, tool-pairing repaired, timestamps backfilled). → `2026-05-30-session-editor-design`
- **Delete a session** (trash with restore).
- **`session-surgery` skill + CLI** (`surgery extract` / `rewrite`) — summarize or prune a turn-snapped message range → a new resumable session (timestamps backfilled so `claude --resume` accepts it). `--from/--to` accept a uuid **prefix**. → `2026-05-30-session-surgery-skill-design`, `2026-06-18-surgery-rewrite-parity-design`

## Lineage

- **Linked / forked sessions** — sidebar badges (`⮌ from …`, `⑂ N`) + a transcript banner, from CC's `forkedFrom`. → `2026-06-14-linked-sessions-design`
- **Cross-session copy lineage** — inferred from shared message-uuid overlap (handles background-restart / manual-`/compact` copies that split a session across files); bidirectional badges + banner. → `2026-06-17-compaction-lineage-design`
- **Inline "forked from here" markers** — at the deepest copied message (the real branch point) in the parent session. → `2026-06-17-fork-point-markers-design`
- **Lineage tree** — on-demand overlay showing a session's whole fork/copy family as an indented tree (`◀ origin` root, `⑂ forked at "…"` / `⧉ copied · N shared` edges, `● you`), opened from the transcript `⑂ Tree` button or the clickable sidebar `⑂ N`/`⧉ N` badges; click a node to open it. A branch label links to the **parent session scrolled to + flashing the branch message** (matched across merged assistant turns); it's plain text with an explanatory tooltip when that point sits on an edited-away branch the transcript doesn't render. → `2026-06-24-lineage-tree-design`

## Harvest (read-only session mining)

- **`session-harvest` skill + `surgery extract --outline [--all-segments]`** — outline-first navigation (compact one-line-per-turn map with 8-char short ids + salient tool args, `· compaction summary` tags), drill down by uuid-prefix range, classify spans → produce a **skill / kb entry / script / summary**. Ranks candidates by reuse over novelty; checks for an already-written distillation before proposing kb/summary. Read-only re sessions. → `2026-06-18-session-harvest-design`, `-harvest-all-segments-`, `-sharpen-harvest-tooling-`
- Outline & drill-down clean machinery: command echoes → `/cmd`, task-notifications collapsed, slash-command/empty turns skipped, embedded skill prompts elided. → `2026-06-18-clean-noisy-messages-design`, `-elide-skill-prompts-design`

## Multi-window

- **Multiple windows** (⌘N + native menu) — one per macOS Space; per-window live-tail isolation (watcher + events scoped by window label). → `2026-06-14-multi-window-design`
- **One terminal per session, app-wide** — the PTY pool is keyed by session (not window), so a session opened in the terminal of one window can't be opened in another; the second window shows "already open in another window" + a **Switch to it** button. Prevents two `claude --resume` processes corrupting one `.jsonl`. → `2026-06-22-terminal-corruption-guard-design`
- **Shared state across windows** — a single backend poller computes discovery + the activity scan once (app-globally, single-flight cache) and broadcasts `activity-updated` / `sessions-changed` to every window, so N windows cost ≈ one; windows listen instead of each polling. Unread (`lastViewed`) syncs across windows via the `storage` event. (Spawned windows are granted the event capability — `windows: ["main", "win-*"]` — which is what makes any cross-window event delivery work at all.) → `2026-06-22-shared-state-poller-design`

## Embedded terminal

- **Manager | Terminal tabs** in the session pane (sidebar always visible). The Terminal tab runs the real interactive `claude --resume <open session>` in a PTY (xterm.js + `portable-pty`) — full TUI, exactly like Ghostty. One persistent terminal per session, **spawned lazily** only when a session's terminal first becomes visible; uses the user's bypass alias (`claude-tracked --dangerously-skip-permissions`). The Manager/Terminal toggle lives in the session header (no separate tab row); **⌘E** toggles tabs, **⌘B** the sidebar. (A bundled-app launch rebuilds `PATH` from a login shell and pins `TERM`/`COLORTERM` so `claude` resolves and renders in color.) A **green dot** on the Terminal tab button marks a live terminal for the open session; a hover-**✕** closes it (kills `claude`, leaving the pane's `[process exited]` + Restart). The terminal is **themed with the app** (`lib/terminalTheme.ts`: bg/fg/cursor from the palette + GitHub's 16-colour ANSI set, re-pushed on change — xterm renders to a canvas and can't read the `--color-*` variables). Claude paints its own UI in 24-bit colour and ignores that palette, so set **claude's own theme to `auto`** (`/theme`) and it matches: `auto` queries the terminal's background via OSC 11, which xterm answers with whatever `terminalTheme` set — so claude follows the app theme too. (Before this, xterm always reported the hardcoded `#0a0a0a` and `auto` could only ever resolve dark.) The ANSI set is what keeps everything *else* — git, ls, test runners — readable. → `2026-06-19-terminal-tab-design`, `2026-07-08-close-terminal-session-design`
- **New session** — a `+ New` sidebar popover (paste a directory path, or create in the selected project's directory) spawns a fresh `claude --session-id` in that cwd. Manager and Terminal share one open session, so they never diverge; the new session surfaces in the sidebar/Manager once claude writes the first turn (polled, since the file doesn't exist until then). → `2026-06-21-new-session-design`

## Live tail & scrolling

- Open sessions **live-tail** their file — new turns stream in (so a session being written, incl. by the embedded terminal, updates in real time).
- **Tail-follow** — auto-scroll to a new turn only when the user is pinned to the bottom; a scrolled-up reader is left alone. Scroll-to-bottom restored when returning from the Terminal tab.

## Customization

- **Settings panel** (`⚙` in the sidebar header, or `⌘,`) backed by a JSON config file — location resolves `$CLAUDE_CODE_MANAGER_CONFIG_DIR` → `$XDG_CONFIG_HOME/claude-code-manager` → `~/.config/claude-code-manager` (`config.json`). Covers **theme** (GitHub Dark / Light — applied by setting the 14 `--color-*` vars live), **typography** (font size 12–18 via the root rem; a `compact` density that shrinks Tailwind's `--spacing` token globally; UI + Mono font choices — presets or a custom family, the mono font also re-applied to the live terminal), and **behavior** (compact / details / sidebar width + collapsed are set-and-stick config; a launch-only default tab). Changes apply live and broadcast to every window (`config-changed`); load is merge-over-defaults + clamp, save is atomic. → `2026-06-28-ui-customization-design`

## Feedback

- **Inline loading indicators** — a shared `<Spinner>` atom marks every async action while it runs: fork / edit-save / delete-message (on the clicked button), sidebar delete / rename / new-session, the terminal's "Starting claude…" overlay, plus session-open and search. → `2026-06-21-loading-indicators-design`
- **Session status indicators** — sidebar dots driven by a lightweight focus-gated poll (`session_activity`, tail-reads recent files): a green pulsing dot when claude is mid-turn (steady through thinking pauses — keyed on `awaiting_input` turn-completeness, not write recency), and an accent dot + bold title for **unread** (your turn, newer than when you last opened it). "Your turn" is `Event::awaits_user`: a completed `end_turn`, **or** a turn parked on a user-blocking tool (`AskUserQuestion` / `ExitPlanMode`) — those stop with `tool_use`, so without it a pending question read as claude still working and lost its dot entirely once the staleness window passed. One predicate, shared by the tail probe and the full parse, since the sidebar reads whichever is available. → `2026-06-22-session-indicators-design`

## Conventions & dev notes

- Workflow per feature: **brainstorm → spec → plan → execute → finish → user-directed merge** (`--no-ff`, tests green, delete branch).
- **Never launch the app from automation** — the user keeps it open across Spaces; `make dev` uses `--no-watch` (Rust changes need a manual restart; frontend hot-reloads via Vite).
- `~/.claude/settings.json` has `cleanupPeriodDays: 36500` so sessions aren't auto-pruned.
- Skills live in `skills/` (`session-surgery`, `session-harvest`).

## Deferred / parked

- **`feat/chat-webui`** (branch, not on `main`) — in-webui agentic chat by driving headless `claude`; parked (custom chat UI needed too much refinement) and **superseded by the embedded terminal tab**. Kept as a fallback.
- Other deferred items: retroactive fork-link backfill (proven unsafe); live-session detection on edit/delete; session-editor later phases.
