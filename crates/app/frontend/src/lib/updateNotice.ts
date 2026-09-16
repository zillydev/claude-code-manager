/** How a Homebrew install upgrades; the app doesn't upgrade itself. */
export const UPGRADE_COMMAND = "brew upgrade --cask claude-code-manager";

/** The release the sidebar note shows: the newest known, unless that one was dismissed. */
export function noteVersion(latest: string | null, dismissed: string | null): string | null {
  return latest !== null && latest !== dismissed ? latest : null;
}
