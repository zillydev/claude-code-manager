import { describe, it, expect } from "vitest";
import { noteVersion, UPGRADE_COMMAND } from "./updateNotice";

describe("noteVersion", () => {
  it("shows nothing when no newer release is known", () => {
    expect(noteVersion(null, null)).toBeNull();
    expect(noteVersion(null, "0.1.3")).toBeNull();
  });

  it("shows a release that wasn't dismissed", () => {
    expect(noteVersion("0.1.3", null)).toBe("0.1.3");
  });

  it("hides the release that was dismissed", () => {
    expect(noteVersion("0.1.3", "0.1.3")).toBeNull();
  });

  it("shows a newer release than the dismissed one", () => {
    expect(noteVersion("0.1.4", "0.1.3")).toBe("0.1.4");
  });
});

describe("UPGRADE_COMMAND", () => {
  it("upgrades the Homebrew cask", () => {
    expect(UPGRADE_COMMAND).toBe("brew upgrade --cask claude-code-manager");
  });
});
