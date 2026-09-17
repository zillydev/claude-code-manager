# Precise status as a default-on Claude Code plugin — design

## Problem

Precise session status (Claude Code's own hooks reporting each session's state) is the better signal, but it is opt-in behind a Settings toggle that writes nine hook entries into `~/.claude/settings.json`. Few people will find the toggle, and the delivery has two problems that rule it out as a default:

- Uninstalling the app leaves the entries behind. `brew uninstall --zap` deletes the relay script from the config dir, so every Claude session then shows a *hook error* on each event; a plain uninstall leaves the relay running forever.
- Nothing about it is visible in Claude Code itself: the hooks are anonymous entries in a settings file.

Claude Code's documented way to distribute hooks is a plugin. A spike (throwaway, isolated `CLAUDE_CONFIG_DIR`) established that:

- `claude plugin marketplace add <dir>` and `claude plugin install <plugin>@<marketplace> --json` run without prompts; `plugin list --json` reports `enabled`; `plugin uninstall --json` and `marketplace remove` reverse them. Install writes only `extraKnownMarketplaces` and `enabledPlugins` to `settings.json`.
- Plugin hooks fire with the same payload the relay reads (`hook_event_name`, `session_id`, `source`, `tool_name`), before login even.
- `plugin list --json` takes about 0.14 s, cheap enough for launch and for opening Settings.
- A directory-sourced marketplace runs the plugin from that directory. Deleting it makes Claude Code skip the plugin silently ("cache-miss", zero hooks registered) — no hook errors.
- Editing the directory's `hooks.json` or relay takes effect in the next session without reinstalling (the listed version stays stale — cosmetic).
- Re-adding the same marketplace name from another path silently re-points it.

## Decisions

- **The plugin.** `crates/app/plugin/` is a marketplace named `claude-code-manager` holding one plugin, `ccm-status`: `.claude-plugin/plugin.json`, `hooks/hooks.json` registering today's nine events (`SessionStart`, `SessionEnd`, `UserPromptSubmit`, `Stop`, `StopFailure`, `PermissionRequest`, `Notification`, and `PreToolUse`/`PostToolUse` matched to `AskUserQuestion|ExitPlanMode`), and `relay.py` — today's relay script, moved out of the Rust string unchanged (no prompt or message content is relayed; it appends to `hook-events.ndjson` in the config dir). The command is `/usr/bin/python3 "${CLAUDE_PLUGIN_ROOT}/relay.py"`, so it needs no execute bit.
- **Shipped in the bundle.** `tauri.conf.json` `bundle.resources` copies it to `Contents/Resources/plugin/`; the app finds it through the resource dir. Deleting the app turns the plugin inert without errors. An app upgrade replaces the files in place, and new sessions use them.
- **Driven through the CLI.** Install: `marketplace add <bundled dir>`, then `install ccm-status@claude-code-manager --scope user --json`. Uninstall: `uninstall ccm-status@claude-code-manager --json`, then `marketplace remove claude-code-manager`. Installed: `plugin list --json` contains that id with `enabled: true`. `claude` is resolved the way terminals resolve it (login-shell PATH, interactive-shell PATH, standard install dirs) but never through the `claude-tracked` wrapper. Each command runs with a 30 s limit; non-zero exit or unparseable output is an error carrying the CLI's message.
- **The choice, owned by the backend.** `config.json` gains `integrations.preciseStatus`: `unset` (default), `auto` (enabled by the app on its own), `on` (chosen, or migrated), `off`. `set_config` always keeps the on-disk value, so a window's stale config can never reset it; the frontend never writes it.
- **At launch** (installed app only — debug builds never install on their own, since they would re-point the marketplace to a build folder), a background thread decides with a pure `launch_action`:
  - Legacy `settings.json` entries present → install the plugin, then remove the legacy entries (the existing exact-inverse removal and one-time backup), set `on`. A failed install keeps the legacy entries and retries next launch.
  - `unset`, `claude` found, and Apple's developer tools present (`xcode-select -p` succeeds, so `/usr/bin/python3` runs instead of opening an install dialog) → install, set `auto`, emit `precise-status-auto`.
  - `auto` or `on` but the plugin isn't installed and enabled → set `off`: removal in `/plugin` is a choice, and the app never reinstalls over it.
  - `off` → nothing. Any failure → no state change, silent; `unset` retries next launch.
- **The marketplace path.** `plugin marketplace list --json` gives the `claude-code-manager` marketplace's recorded `path`; `plugin list` still reports the plugin enabled when that directory is gone. When it no longer exists (the app moved), launch re-adds the marketplace from the current bundle. An existing directory is never re-pointed, so a dev build's toggle and the installed app don't fight.
- **Settings toggle.** On → install, `on`; off → uninstall, `off`. Its checked state comes from `plugin list`, fetched when Settings opens. Enabling without developer tools shows the reason ("needs Apple's command line tools: `xcode-select --install`").
- **Notice.** While the choice is `auto` and not dismissed, the sidebar foot shows a one-time note above the update note: "Precise session status is on — ccm added its status plugin to Claude Code." with a **Settings** link and ✕. Dismissal persists in `usePersistentState("cm:preciseStatusNoticeDismissed")`, shared across windows. Windows learn the choice from `precise_status_choice` on mount and `precise-status-auto` after.
- **Unchanged.** The events file, the poller's tail and fold, and the witness-not-authority rule. The legacy relay file in the config dir is left in place: sessions started before migration may still invoke it.

## Rejected

- **Keep writing hooks into `settings.json`, default-on.** Invisible in Claude Code, and uninstalling the app leaves hook errors in every session.
- **A GitHub-hosted marketplace.** Needs network to install, updates separately from the app, and keeps running after the app is gone.
- **Ask on first launch.** The chosen default is automatic with a notice; the toggle and `/plugin` both undo it, and the choice to undo is remembered.
- **A compiled relay instead of python.** Removes the developer-tools condition, but adds a second universal binary to the bundle for the few Claude Code users without the tools.

## Testing

- Unit (Rust): `launch_action` for every row above (legacy present, each choice, tools/claude missing, plugin missing); command argument construction; parsing `plugin list --json` (installed and enabled, installed and disabled, absent, malformed); `set_config` keeping the on-disk choice; `preciseStatus` round-trip and default; `hooks.json` in the plugin matching `HOOK_EVENTS`.
- Unit (TS): notice visibility (`auto` and not dismissed only).
- Gate: `cargo clippy --workspace --all-targets -- -D warnings`, both test suites, both builds, `make build`, and `claude plugin validate crates/app/plugin`.
- Manual (installed app via `make install`, real config): the migration from the current `settings.json` entries (they disappear, the plugin appears in `/plugin`, dots keep working); toggle off and on; removing the plugin in `/plugin` then relaunching (stays off); a fresh `unset` config (plugin installed, notice shown once across windows).
