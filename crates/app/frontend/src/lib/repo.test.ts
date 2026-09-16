import { describe, it, expect } from "vitest";
import { releaseUrl } from "./repo";

describe("releaseUrl", () => {
  it("is the release's tag page, which carries the v prefix", () => {
    expect(releaseUrl("0.1.3")).toBe("https://github.com/zillydev/claude-code-manager/releases/tag/v0.1.3");
  });
});
