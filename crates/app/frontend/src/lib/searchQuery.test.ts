import { describe, it, expect } from "vitest";
import { parseSearchQuery, isRunnable } from "./searchQuery";

describe("parseSearchQuery", () => {
  it("splits operators from free text", () => {
    const q = parseSearchQuery("in:mono from:user rate limit");
    expect(q.project).toBe("mono");
    expect(q.role).toBe("user");
    expect(q.text).toBe("rate limit");
    expect(q.errors).toEqual([]);
  });

  it("supports quoted values and quoted text", () => {
    const q = parseSearchQuery('in:"my proj" "exact phrase"');
    expect(q.project).toBe("my proj");
    expect(q.text).toBe("exact phrase");
  });

  it("treats unknown keys as literal text (Discord behaviour)", () => {
    const q = parseSearchQuery("foo:bar hello");
    expect(q.text).toBe("foo:bar hello");
  });

  it("repeats tag: as AND; last-wins for single-value keys", () => {
    const q = parseSearchQuery("tag:wip tag:api in:a in:b");
    expect(q.tags).toEqual(["wip", "api"]);
    expect(q.project).toBe("b");
  });

  it("parses dates to local midnight; after inclusive semantics live downstream", () => {
    const q = parseSearchQuery("after:2026-07-01");
    expect(q.after).toBe(Math.floor(new Date(2026, 6, 1).getTime() / 1000));
  });

  it("surfaces malformed values as inline errors, never throws", () => {
    const q = parseSearchQuery("before:july from:bot is:starred has:image");
    expect(q.errors).toHaveLength(4);
  });

  it("is:bookmarked and has:link set flags", () => {
    const q = parseSearchQuery("is:bookmarked has:link");
    expect(q.bookmarked).toBe(true);
    expect(q.hasLink).toBe(true);
  });
});

describe("isRunnable", () => {
  it("rejects an empty query (no whole-disk dump)", () => {
    expect(isRunnable(parseSearchQuery("   "))).toBe(false);
  });
  it("any filter alone is runnable", () => {
    expect(isRunnable(parseSearchQuery("tool:Bash"))).toBe(true);
  });
});
