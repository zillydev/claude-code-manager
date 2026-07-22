# claude-code-manager

A desktop app and terminal UI for browsing, searching, editing, and resuming
[Claude Code](https://claude.com/claude-code) sessions.

Claude Code stores every session as a JSON-lines file under
`~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`, linked into a tree by
`parentUuid`. That format is great for the CLI and awkward for everything else:
you can't easily find the session where you solved something, fork a
conversation before it went sideways, or trim a bloated context. This reads
those files directly and gives you a real interface over them.

> **Safety first:** every fork / edit / delete / surgery writes a **new** session
> file. Your original transcripts are never mutated.

## What it does

**Browse & read**
- Session list across all projects — title, project, token cost, last activity —
  with project, tag, and time filters, plus full-text search across transcripts.
- Rendered transcript: merged assistant turns, GFM markdown, expandable tool
  calls with full input/output, and in-session find (⌘F) with regex.
- Renders pre-compaction history: a compacted session's earlier trees are shown
  with segment dividers instead of silently disappearing.

**Edit & branch (copy-on-edit, always)**
- **Fork from any message** into a new resumable session. Forking a compacted
  session trims the copy to Claude's *current* context segment, so the fork
  doesn't replay discarded history and blow the context window on resume.
- Edit or delete a message, producing a new session.
- **Session surgery** (CLI + skill): summarize or prune a range of messages to
  reclaim context, emitting a new session Claude Code will happily resume.

**Organize**
- Tags (colored, global), a local alias (a rename that doesn't touch Claude's
  own files), and per-session notes. Stored separately from the `.jsonl`.

**Live terminal**
- A built-in terminal tab runs the real `claude --resume <session>` in a PTY, so
  you can read a session and continue it in the same window. One terminal per
  session, app-wide, so two processes can never corrupt the same file.

**Lineage**
- Fork/copy relationships are detected (including Claude's own background
  session copies) and shown as an explorable tree.

## Requirements

- **macOS** (the desktop app is a macOS bundle; the TUI is Unix-friendly)
- [Rust](https://rustup.rs) (stable)
- [Node.js](https://nodejs.org) 20+ (for the webui frontend)
- [Claude Code](https://claude.com/claude-code) — this reads the sessions it writes
- [`cargo-tauri`](https://tauri.app) for the desktop app: `cargo install tauri-cli --version '^2'`

## Install

```bash
git clone https://github.com/zillydev/claude-code-manager.git
cd claude-code-manager

make install   # builds the macOS app and installs it to /Applications
```

Other entry points:

```bash
make tui       # terminal UI
make webui     # desktop app in dev mode (Vite HMR; PORT=3000 to change port)
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
- `session-meta.json` — your tags, aliases, and notes, keyed by session id.

Claude Code's own files under `~/.claude/` are never written to by these.

## Skills

Two [Claude Code skills](https://docs.claude.com/en/docs/claude-code/skills) ship
in [`skills/`](skills/), driving the `surgery` CLI:

- **`session-surgery`** — shrink a session's context by summarizing or pruning a
  range of messages into a new resumable session.
- **`session-harvest`** — read-only mining of a session into a skill, knowledge
  base entry, script, or summary.

To use them, symlink into your skills directory and make the binary reachable:

```bash
ln -s "$PWD/skills/session-surgery" ~/.claude/skills/session-surgery
cargo install --path crates/tui        # puts `claude-code-manager` on PATH
```

## Architecture

A Cargo workspace with a shared core, so the TUI and the desktop app can't drift:

| Crate | What it is |
| --- | --- |
| `crates/core` | Parsing, tree/lineage resolution, rendering, the copy-on-edit write pipeline, surgery |
| `crates/tui` | ratatui terminal UI + the `surgery` CLI (binary: `claude-code-manager`) |
| `crates/webui` | Tauri 2 desktop app; React 19 + TypeScript + Tailwind frontend |

[`FEATURES.md`](FEATURES.md) is the living catalog of what exists, with each
entry linking to its design doc in [`docs/superpowers/specs/`](docs/superpowers/specs/).

## Development

```bash
cargo test                                  # Rust
cargo clippy --workspace --all-targets      # lints (CI enforces -D warnings)

cd crates/webui/frontend
npx tsc --noEmit && npx vitest run          # frontend typecheck + tests
```

The codebase is deliberately hand-formatted, so `cargo fmt` is **not** enforced.

## License

[MIT](LICENSE)
