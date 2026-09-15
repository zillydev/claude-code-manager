# Changelog

What changed in each release, written for people using the app. Build-time detail (specs, plans, CI, refactors) is deliberately left out — the commit history has it.

> **Maintenance:** add entries under **Unreleased** on the feature branch, alongside the `FEATURES.md` update. At release time that heading becomes the version. `release.yml` publishes the section matching the tag and fails if it finds none, so a release can't ship without notes.

## Unreleased

**Added**

- **Precise status via Claude Code hooks** (opt-in, Settings → Integrations) — Claude itself now reports each session's state, so the sidebar and the published status file can say **needs you** the instant a question, permission prompt, or plan appears, and **your turn** within seconds of a reply finishing — no more waiting out the quiet-terminal hold. It installs a small relay into `~/.claude/settings.json` (your existing hooks are untouched; a one-time backup is kept, and switching off removes exactly what was added). Sessions started before toggling keep the inferred status.

- **About section in Settings** — shows the running build's version, and opens the repository, that version's release notes, and the issue tracker.

**Fixed**

- **Question cards show every answer.** A chosen option whose label contains a comma is highlighted again (Claude Code quotes such labels inside the answer sentence, and the card's parser stopped at the first quote), and an answer you typed instead of picking — including a notes-only answer — now appears as its own accented row under the question, rather than the card looking unanswered.
- **Pressing Esc before Claude's first token no longer leaves the session "running".** Claude Code writes no interrupt marker when there was nothing to interrupt, and no hook fires on Esc — so ccm now treats the Esc / Ctrl-C you press in its own terminal as the end of the turn.
- **Interrupting Claude while nothing was streaming no longer leaves the session "running".** Esc during a queued prompt or a task notification writes the interrupt marker without the message id the previous detection relied on — a third of all interrupts. The marker text itself now ends the turn.
- **A global-find result takes you to the message.** Clicking one often did nothing, for several unrelated reasons. Results included lines Claude Code writes as user messages but the transcript never shows — skill bodies, slash-command caveats, and a slash command's own stdout (`Set model to …`), which alone was 104 of 355 unreachable results on a real corpus — so there was nothing to scroll to; search now asks the same question the renderer does, and stops offering them. Slash-command, `!` shell and task-notification turns *are* shown but carried no anchor to scroll to (one subagent-heavy session had 180 unreachable task notifications); they do now. And a message on a branch the session rewound away from is in the file but on no screen: the transcript now says so and offers **Show this branch**, which re-renders the session along that path and scrolls to the message, with a banner back to the current branch. A message Compact mode is filtering still offers Show all.

- **A session waiting on a question shows as unread, not running.** Claude Code writes nothing to its session file while a question is on screen, and a live terminal only proves the process exists — so for a session ccm is running, the terminal itself now answers, by attribution: output that follows your own keystroke or a resize is the TUI answering *you* (browsing a question's options, typing, scrolling), while a real burst Claude wasn't asked for — a spinner tick, streamed text — is work. That moment of falling silent is also what dates the wait, since the file still ends on your own prompt: the session goes unread, the way a finished turn does, and clears when you look at it — including the session you have open, if its window is behind another or on a Space you're not on. A long build keeps its running indicator throughout.

**Changed**

- **`!` shell commands render as a terminal block.** Running `!cmd` in Claude Code wrote two messages showing raw `<bash-input>` / `<bash-stdout>` tags with HTML-escaped text (`&amp;` everywhere). They now merge into one block: the command on a `$` line with its output beneath, unescaped, stderr in red.
- **A long unbroken string no longer breaks the layout.** A message containing a long URL (or any token with nowhere to wrap) stretched its bubble past the pane instead of wrapping; find results could do the same in the sidebar.
- **Bash commands render as shell scripts.** Half of them are multi-line, and as JSON every newline arrived as a literal `\n` in one long string. The script now reads as lines, syntax-highlighted, with the description and timeout as `#` comments above it.
- **Questions render as question cards.** An `AskUserQuestion` call showed its raw JSON; it now reads as the question with its options listed, and the option you chose is highlighted.
- **File edits render as diffs.** `Edit`, `Write` and `MultiEdit` used to show their raw JSON input, with the before/after text collapsed onto one `\n`-escaped line; they now show `-`/`+` lines like the terminal does, syntax-highlighted, with the changed lines tinted. Other tools keep the JSON view.

## v0.1.2 — 2026-08-22

**Added**

- **Right-click context menus** on sessions and messages, so the actions behind the `⋯` menus are reachable where you're already pointing.

**Fixed**

- **Sessions no longer stick on "running."** Stopping a turn, cancelling one mid-flight, or waiting through a long tool call each left the green dot burning indefinitely; a session that went quiet without finishing also could never be marked read.
- **API errors render** in the transcript instead of leaving a blank gap where the reply should be.
- **The terminal finds `claude`** when your `PATH` is set in `.zshrc` rather than `.zprofile`.

## v0.1.1 — 2026-08-21

**Added**

- **Session status publishing** (opt-in, Settings → Integrations) — writes `session-status.json` to the config directory for other tools to read: which sessions are running, which want your attention, and which macOS window each one is running in. Built for status bars; the README documents the format.
- **Default project on launch** (Settings → Behavior) — the sidebar starts filtered to a project you pick, instead of showing everything.

**Changed**

- **The sidebar's project / tag / time filters are one consistent set of menus.** Project and tags gained type-to-filter over a scrolling list with arrow-key navigation, and the selected entry pins to the top; time moved behind a clock icon; a single ✕ clears everything. The old native dropdowns ignored the app's theme entirely.
- **The font pickers** in Settings use the same menus, and preview each font in its own face.
- **The selected session** in the sidebar is clearly marked — an accent tint and rail, rather than the same grey as hovering.
- **The tags button is always available**, instead of appearing only once a tag exists — which had hidden the one place tags can be created.

**Fixed**

- **A session's branch** now shows the branch it was last working on. It used to show the branch it *started* on, which for a long-lived session could be days out of date.
- **A reply that lands while you're on another Space** is treated as unread. Having the session open in a window you can't see counted as reading it.

**Install**

- **Homebrew** — `brew tap zillydev/tap && brew trust zillydev/tap && brew install --cask claude-code-manager`. Releases update the cask automatically.

## v0.1.0 — 2026-08-20

First release. A desktop app over the session files Claude Code writes:

- **Browse** every session across all projects, with live status dots for what's running or waiting on you, a rendered transcript (markdown, syntax highlighting, expandable tool calls, inline images), and a compact mode that collapses tool churn.
- **Search** — an in-session find bar (⌘F) with regex and match highlighting, and a Discord-style search across all sessions (⌘⇧F) with operators like `in:`, `from:`, `before:`, `tag:` and `is:bookmarked`.
- **Branch** — fork any message into a new resumable session, copied verbatim so nothing is lost, plus `session-surgery` for summarizing or pruning a range to reclaim context.
- **Organize** — tags, per-session aliases and notes, and message bookmarks, all stored separately from Claude's own files.
- **Continue** — a built-in terminal tab running the real `claude --resume`, one per session app-wide so two processes can't corrupt the same file.
- **Lineage** — fork and copy relationships detected and shown as an explorable tree.
