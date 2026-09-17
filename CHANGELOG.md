# Changelog

What changed in each release, written for people using the app. Build-time detail (specs, plans, CI, refactors) is deliberately left out — the commit history has it.

> **Maintenance:** add entries under **Unreleased** on the feature branch, alongside the `FEATURES.md` update. At release time that heading becomes the version. `release.yml` publishes the section matching the tag and fails if it finds none, so a release can't ship without notes.

## Unreleased

**Added**

- **Diffs for files a shell command changed.** When Claude edits files with a command — `sed`, a script, a heredoc — rather than its Edit tool, Claude Code now records what changed, and the transcript shows it: each file's diff appears after the command's output, highlighted like an Edit, and the collapsed call shows how many files and lines changed (`2 files +3 −1`). In-session find searches these diffs too. In these diffs and in Edit diffs, a changed line now also highlights the exact words that changed, so a one-word edit in a long paragraph stands out. Sessions from Claude Code 2.1.270 on.
- **A notice when a new version is out.** The app checks GitHub for a newer release now and then, and when there is one, the foot of the sidebar says so, with the command that upgrades it — `brew upgrade --cask claude-code-manager` — and a button to copy it. Click the version to read its release notes. Dismiss it with ✕ and it stays hidden until the next release; Settings → About keeps showing it.
- **An Open tab for running terminals.** A new sidebar tab (⌘4) lists every session with Claude running in a terminal, across all your windows, and which window each is in. Close one with ✕ — it asks first if Claude is mid-turn — or close all the idle ones at once.
- **Session links.** "Copy link" in a session's menu copies a `claude-code-manager://session/…` link; clicking it — say, in an Obsidian note — opens the app on that session, launching it if it isn't running and bringing forward the window that already has it. Links open the installed app, not a development build.
- **A warning before closing on running work.** Closing a window or quitting with Claude still working in one of its terminals now asks first, listing those sessions; closing it would stop them mid-turn. Idle terminals close as before, since their sessions resume from where they were. (Quitting from the Dock or logging out doesn't ask.)
- **A clear button in global find.** A ✕ in the search field clears the query and its results at once and leaves the cursor in the field, with the search options showing again.
- **Precise status via Claude Code hooks** (opt-in, Settings → Integrations) — Claude itself now reports each session's state, so the sidebar and the published status file can say **needs you** the instant a question, permission prompt, or plan appears, and **your turn** within seconds of a reply finishing — no more waiting out the quiet-terminal hold. It installs a small relay into `~/.claude/settings.json` (your existing hooks are untouched; a one-time backup is kept, and switching off removes exactly what was added). Sessions started before toggling keep the inferred status.

- **About section in Settings** — shows the running build's version, and opens the repository, that version's release notes, and the issue tracker.

**Fixed**

- **A fork opens as soon as you create it.** Forking stayed on the original session, and the new one — which keeps the forked message's timestamps — sorted among old sessions, far below where you were looking. Forking now opens the fork, and the sidebar scrolls to its row wherever it sorts.
- **A session's title is its latest prompt, not an out-of-date one.** The sidebar and title bar took the title from a record Claude Code writes only now and then, so it lagged a turn behind while Claude worked, and stuck on an older prompt for good when a session stopped before the next write — 45 of 300 recent sessions showed a stale one. The title now comes from the conversation itself, including prompts with images, `/commands`, `!` commands and messages queued while Claude works, and a finished background task can no longer become the title. The title bar also updates as you go, rather than only when the session is reopened.
- **A slow search can't overwrite a newer one.** Running a second search before the first finished could leave the first one's results on screen once it caught up; only the latest search's results are shown now.
- **Question cards show every answer.** A chosen option whose label contains a comma is highlighted again (Claude Code quotes such labels inside the answer sentence, and the card's parser stopped at the first quote), and an answer you typed instead of picking — including a notes-only answer — now appears as its own accented row under the question, rather than the card looking unanswered.
- **Pressing Esc before Claude's first token no longer leaves the session "running".** Claude Code writes no interrupt marker when there was nothing to interrupt, and no hook fires on Esc — so ccm now treats the Esc / Ctrl-C you press in its own terminal as the end of the turn.
- **Interrupting Claude while nothing was streaming no longer leaves the session "running".** Esc during a queued prompt or a task notification writes the interrupt marker without the message id the previous detection relied on — a third of all interrupts. The marker text itself now ends the turn.
- **A global-find result takes you to the message.** Clicking one often did nothing, for several unrelated reasons. Results included lines Claude Code writes as user messages but the transcript never shows — skill bodies, slash-command caveats, and a slash command's own stdout (`Set model to …`), which alone was 104 of 355 unreachable results on a real corpus — so there was nothing to scroll to; search now asks the same question the renderer does, and stops offering them. Slash-command, `!` shell and task-notification turns *are* shown but carried no anchor to scroll to (one subagent-heavy session had 180 unreachable task notifications); they do now. And a message on a branch the session rewound away from is in the file but on no screen: the transcript now says so and offers **Show this branch**, which re-renders the session along that path and scrolls to the message, with a banner back to the current branch. A message Compact mode is filtering still offers Show all.

- **A session waiting on a question shows as unread, not running.** Claude Code writes nothing to its session file while a question is on screen, and a live terminal only proves the process exists — so for a session ccm is running, the terminal itself now answers, by attribution: output that follows your own keystroke or a resize is the TUI answering *you* (browsing a question's options, typing, scrolling), while a real burst Claude wasn't asked for — a spinner tick, streamed text — is work. That moment of falling silent is also what dates the wait, since the file still ends on your own prompt: the session goes unread, the way a finished turn does, and clears when you look at it — including the session you have open, if its window is behind another or on a Space you're not on. A long build keeps its running indicator throughout.

**Changed**

- **A session reopens where you were reading.** Scroll up in a session, switch away — or quit — and it now comes back to the same message instead of jumping to the latest. A session you left at the bottom still opens at the latest, new turns included, and opening one from a search result, bookmark or fork still goes to that message. Positions hold across compact mode and window resizes, and the last 300 sessions are remembered.
- **A session reopens on the tab you left it on.** Switching away from a session you were using in its Terminal and coming back used to land in Manager; it now returns to the Terminal as long as claude is still running there. A session you left on Manager comes back to Manager, and one whose terminal has stopped opens in Manager rather than starting claude again. Remembered per window, until you quit.
- **Search options follow what you type, and `tool:` lists your tools.** The options list in global find used to vanish the moment you typed a letter; it now narrows to the operators that word could become (`to` → `tool:`, `link` → `has:link`) and picking one replaces the word. After `tool:` it lists every tool your sessions have called, most used first, matching anywhere in the name — so `run` finds `mcp__tabularis__run_query` without remembering the prefix.
- **Tool results show what the call did.** A search like `tool:Bash` or `tool:AskUserQuestion` returned a wall of identical "(tool: Bash)" cards. Each card now shows the call itself: the command it ran, the question it asked, the file it edited, the pattern it searched for. Bookmarked tool calls read the same way, and a collapsed tool call in the transcript names what it did instead of showing raw JSON. Long file paths keep their file name.
- **The find match you're on stands out.** It was the same amber as every other match, just darker, so it was hard to pick out; it is now a solid orange with its own text colour, and its mark in the scrollbar is orange too.
- **A global-find result shows you the text you searched for.** Opening a result used to land on the right message but leave you to find the words in it yourself — hard in a long reply. It now opens the find bar with your search, starting on the match inside that message, so the text is highlighted, scrolled into view, and every other match in the session is a ⌘G away. Your find-bar toggles are left as you set them.
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
