# Update notice — design

## Problem

Nothing in the app says a newer release exists. People who installed through Homebrew keep running an old build until they happen to check GitHub or run `brew upgrade` for another reason.

The app should notice a newer release on its own and say how to get it: `brew upgrade --cask claude-code-manager`. It doesn't upgrade itself.

## Decisions

- **Source.** `GET https://api.github.com/repos/zillydev/claude-code-manager/releases/latest`, which leaves out drafts and prereleases, so an `-rc` tag never prompts an upgrade. Only `tag_name` is read. GitHub rejects requests without a `User-Agent`, so it is set to the product name.
- **HTTP client.** `ureq` 3 with `default-features = false, features = ["native-tls"]`: blocking, small, and TLS through macOS's Security framework instead of pulling in rustls and ring. ureq still defaults to the rustls provider and to Mozilla's bundled roots, so the agent selects `TlsProvider::NativeTls` (a probe panicked without it) and `RootCerts::PlatformVerifier`, the Keychain's roots, which also trust a TLS-inspecting proxy's CA installed there. The bundled roots are still compiled in: `native-tls-no-default` would drop them, but ureq 3.4 panics on https with it. 10 s global timeout.
- **One check for the whole app.** A backend thread started in `setup` does the checking, not each window, so N windows make one request. The first check runs 10 s after launch. After that the thread wakes every 10 minutes and checks again once 6 h of wall-clock time have passed since the last attempt. Wall-clock matters because a sleeping Mac pauses `thread::sleep`: a plain 6 h sleep could stretch across days on a laptop that stays open.
- **Comparing.** A pure `newer_release(running, tag)` strips a leading `v`, parses the tag as semver, and returns it only when it is strictly newer than the running version (`package_info().version`). An unparseable tag returns nothing.
- **Failures are silent.** No network, a GitHub error, rate limiting (60 requests an hour per IP, unauthenticated) or a malformed body change nothing. The last known newer version stays, and the next attempt waits the full 6 h.
- **State and delivery.** `UpdateState` holds the newest release found, if it is newer. When it changes, the thread emits `update-available` with the version to every window. `update_available` returns it, for windows opened after the event.
- **Dev builds.** Debug builds don't check, so rebuild-and-relaunch loops don't spend the rate limit. `CM_UPDATE_CHECK_AS=<version>` turns the check on in a debug build and uses that version as the running one: `CM_UPDATE_CHECK_AS=0.1.0 make dev` shows the notice. Release builds ignore the variable.
- **Sidebar note.** At the bottom of the sidebar, below the tab panes and on every tab. A title line — "Update available", the version, and a ✕ to dismiss — over the command in a one-line code box: `brew upgrade --cask claude-code-manager`. The version links to that release's page on GitHub (opened in the browser, like About's Release notes). The command is selectable and has a copy button; each word is unbreakable, so a narrow sidebar wraps it between words, never at `--`. No upgrade button. A collapsed sidebar isn't rendered, so the note goes with it.
- **Dismissing.** Per version, in `usePersistentState("cm:dismissedUpdate")`, so dismissing in one window hides it in all of them and after relaunch. A newer release than the dismissed one shows the note again.
- **Settings → About.** Under the version row, an "Update available" row with the version link and the same command box, whenever a newer release is known, dismissed or not.

## Rejected

- **An in-app upgrade button** (running `brew upgrade` or downloading the DMG). Not wanted: the note is enough.
- **Checking from each window's frontend.** A request per window, each on its own timer, and windows could disagree about what's available.
- **`tauri-plugin-updater`.** It replaces the bundle itself and needs signed update artifacts. The app is unsigned and Homebrew already owns the install.
- **`reqwest`.** Pulls in tokio and hyper for one request.

## Testing

- Unit (Rust): `newer_release` for newer, equal, older, a leading `v` or none, a prerelease running version, an unparseable tag. The due check for first attempt, before and after 6 h, and a clock that went backwards. Parsing `tag_name` out of a release body, and a body without it. Which version a release or debug build checks as, with and without `CM_UPDATE_CHECK_AS`. An ignored test that fetches the real endpoint, run once with `--ignored`.
- Unit (TS): the note's visibility for no release, not dismissed, dismissed, and a newer release than the dismissed one.
- Gate: `cargo clippy --workspace --all-targets -- -D warnings`, both test suites, both builds.
- Manual (`CM_UPDATE_CHECK_AS=0.1.0 make dev`): the note appears about 10 s after launch in every window, including one opened afterwards. ✕ hides it in all windows and it stays hidden after relaunch. Settings → About still shows it. Without the variable, no note appears.
