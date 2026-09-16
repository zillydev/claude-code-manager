export const REPO_URL = "https://github.com/zillydev/claude-code-manager";

/** A release's page on GitHub: its notes and downloads. */
export function releaseUrl(version: string): string {
  return `${REPO_URL}/releases/tag/v${version}`;
}
