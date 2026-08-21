# claude-code-manager

A desktop app for browsing, searching, branching, and resuming [Claude Code](https://claude.com/claude-code) sessions.

Claude Code stores every session as a JSON-lines file under `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`, linked into a tree by `parentUuid`. That format is great for the CLI and awkward for everything else: you can't easily find the session where you solved something, fork a conversation before it went sideways, or trim a bloated context. This reads those files directly and gives you a real interface over them.

> **Safety first:** every fork / surgery writes a **new** session file. Your original transcripts are never mutated.

## What it does

**Browse & read**
- Session list across all projects — title, project, token cost, last activity — with project, tag, and time filters, and live status dots for sessions Claude is mid-turn on or that are waiting on you.
- Rendered transcript: merged assistant turns, GFM markdown, syntax-highlighted code, expandable tool calls with full input/output, attached and tool-returned images, and a **Compact** mode that collapses tool churn to just the prompts and replies.
- Renders pre-compaction history: a compacted session's earlier trees are shown with segment dividers instead of silently disappearing.

**Search**
- **In-session find** (⌘F) — a Zed-style bar with case / whole-word / regex toggles, an optional pass over collapsed tool calls and results, match highlighting in place, and an overview ruler marking every hit in the scroll gutter.
- **Global find** (⌘⇧F) — Discord-style search across every session with operators: `in:` `from:` `before:` `after:` `tag:` `tool:` `has:link` `is:bookmarked` `session:`. Results are newest-first cards with highlighted excerpts; clicking one opens that session at that message.
- **Bookmarks** — flag any message, then browse or query them across sessions.

**Branch (copy-on-edit, always)**
- **Fork from any message** into a new resumable session. The fork is a verbatim copy of the file up to that point — nothing is flattened or trimmed — so abandoned branches and pre-compaction history come along, and Claude resumes it exactly as it stood.
- **Session surgery** (CLI + skill): summarize or prune a range of messages to reclaim context, emitting a new session Claude Code will happily resume.

**Organize**
- Tags (colored, global, archivable), a local alias (a rename that doesn't touch Claude's own files), and per-session notes. Stored separately from the `.jsonl`.

**Live terminal**
- A built-in terminal tab runs the real `claude --resume <session>` in a PTY, so you can read a session and continue it in the same window. One terminal per session, app-wide, so two processes can never corrupt the same file.

**Lineage**
- Fork/copy relationships are detected (including Claude's own background session copies) and shown as an explorable tree.

## Requirements

- **macOS** (the app ships as a macOS bundle)
- [Rust](https://rustup.rs) (stable)
- [Node.js](https://nodejs.org) 20+ (for the app's frontend)
- [Claude Code](https://claude.com/claude-code) — this reads the sessions it writes
- [`cargo-tauri`](https://tauri.app) for the desktop app: `cargo install tauri-cli --version '^2'`

## Install

### Homebrew (recommended)

```bash
brew tap zillydev/tap
brew trust zillydev/tap        # Homebrew won't load casks from a third-party tap until you trust it
brew install --cask claude-code-manager
```

The cask clears the quarantine flag for you (see the note below on signing), so the app opens straight away. `brew upgrade --cask claude-code-manager` updates it; `brew uninstall --cask --zap claude-code-manager` removes it along with its config.

### Download a release

Grab the universal macOS `.dmg` (Apple Silicon + Intel) from [Releases](https://github.com/zillydev/claude-code-manager/releases), then drag the app to `/Applications`.

Release builds are **not signed or notarized by Apple** (that needs a paid Apple Developer account), so Gatekeeper blocks them on first launch. Clear the quarantine attribute once:

```bash
xattr -cr "/Applications/Claude Code Manager.app"
```

(Or right-click the app → **Open** → **Open**.)

### Or build from source

Building locally produces no quarantine attribute, so there's nothing to clear:

```bash
git clone https://github.com/zillydev/claude-code-manager.git
cd claude-code-manager

make install   # builds the macOS app and installs it to /Applications
```

Other entry points:

```bash
make dev       # run the app in dev mode (Vite HMR; PORT=3000 to change port)
make app       # build the .app/.dmg bundle without installing
make build     # compile everything (no bundle) — the CI check
make test      # run the Rust test suite
```

## Configuration

Settings and session metadata live in a JSON config directory, resolved in order:

1. `$CLAUDE_CODE_MANAGER_CONFIG_DIR`
2. `$XDG_CONFIG_HOME/claude-code-manager`
3. `~/.config/claude-code-manager`

- `config.json` — theme, typography, density, and behavior defaults.
- `session-meta.json` — your tags, aliases, notes, and bookmarks, keyed by session id.
- `session-status.json` — live session state, written only while **Publish session status** is enabled in Settings (see below).

Claude Code's own files under `~/.claude/` are never written to by these.

## Session status for other tools

With **Publish session status** enabled (Settings → Integrations), ccm keeps `session-status.json` in the config directory current — a small, versioned snapshot other tools can poll:

```json
{
  "version": 1,
  "updated": 1787263639.0,
  "sessions": {
    "59a0dc6a-…": {
      "state": "running",
      "awaiting_input": false,
      "last_event_at": 1787263620,
      "cwd": "/path/to/project",
      "title": "…",
      "window": 61729,
      "hosted": true
    }
  }
}
```

`state` is ccm's opinion — `running` (Claude is mid-turn), `unread` (finished, newer than the last time you opened it), `idle` (finished and seen) — while `awaiting_input` and `last_event_at` are the raw facts, so a consumer can apply its own rules. `window` is the macOS window id (the same integer yabai and similar tools use) when ccm is running that session in a terminal tab, `null` otherwise; `hosted` says which. Only sessions that are running, unread, or hosted appear.

State is derived from the session files themselves — the same 3s poller behind the sidebar's status dots — so it self-corrects and can't be stranded the way Claude Code hook-driven state can. The file is replaced atomically, rewritten only when the contents change, and removed when publishing is switched off or the app exits, so a stale snapshot never outlives the app.

## Skills

Two [Claude Code skills](https://docs.claude.com/en/docs/claude-code/skills) ship in [`skills/`](skills/), driving the `surgery` CLI:

- **`session-surgery`** — shrink a session's context by summarizing or pruning a range of messages into a new resumable session.
- **`session-harvest`** — read-only mining of a session into a skill, knowledge base entry, script, or summary.

To use them, symlink into your skills directory and make the binary reachable:

```bash
ln -s "$PWD/skills/session-surgery" ~/.claude/skills/session-surgery
cargo install --path crates/cli        # puts `claude-code-manager` on PATH
```

## Architecture

A Cargo workspace. All session logic lives in `core`, so the app and the CLI can't drift — and another front-end can be added without touching it:

| Crate | What it is |
| --- | --- |
| `crates/core` | Parsing, tree/lineage resolution, rendering, search, the copy-on-edit write pipeline, surgery |
| `crates/app` | Tauri 2 desktop app; React 19 + TypeScript + Tailwind frontend |
| `crates/cli` | The `claude-code-manager` binary — `surgery` subcommands, driven by the skills |

[`FEATURES.md`](FEATURES.md) is the living catalog of what exists, with each entry linking to its design doc in [`docs/superpowers/specs/`](docs/superpowers/specs/).

## Development

```bash
cargo test                                  # Rust
cargo clippy --workspace --all-targets      # lints (CI enforces -D warnings)

cd crates/app/frontend
npx tsc --noEmit && npx vitest run          # frontend typecheck + tests
```

The codebase is deliberately hand-formatted, so `cargo fmt` is **not** enforced.

### Cutting a release

Version lives in `crates/app/tauri.conf.json` (the bundle version) and the three `crates/*/Cargo.toml`. Bump them together, then tag:

```bash
git tag v0.2.0 && git push origin v0.2.0
```

That triggers [`release.yml`](.github/workflows/release.yml), which builds a universal macOS bundle and publishes it. The workflow refuses to run if the tag doesn't match the version in `tauri.conf.json`, so a release can't ship a mislabelled `.dmg`.

To sign and notarize instead of shipping unsigned, add the `APPLE_*` repo secrets listed in that workflow (requires a paid Apple Developer account); no workflow changes are needed.

## License

[MIT](LICENSE)
