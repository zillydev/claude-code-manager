import { describe, it, expect } from "vitest";
import { activeIndexAfterSearch, seedQuery, startMatchIndex } from "./findHandoff";
import type { FindMatch } from "./findMatches";

// Already in transcript order, as useFind keeps them.
const at = (turnIndex: number, start = 0): FindMatch => ({
  segId: `t${turnIndex}`,
  turnIndex,
  blockIndex: null,
  field: "user-text",
  start,
  end: start + 3,
});

describe("startMatchIndex", () => {
  it("starts on the first match inside the opened message", () => {
    const matches = [at(1), at(4, 10), at(4, 50), at(9)];
    expect(startMatchIndex(matches, 4)).toBe(1);
  });

  it("falls forward to the next match when the message has none", () => {
    // Aa or whole-word can reject the occurrence global find matched.
    expect(startMatchIndex([at(1), at(6), at(9)], 4)).toBe(1);
  });

  it("falls back to the last match when nothing comes after the message", () => {
    expect(startMatchIndex([at(1), at(2)], 4)).toBe(1);
  });

  it("is 0 when there are no matches", () => {
    expect(startMatchIndex([], 4)).toBe(0);
  });
});

describe("seedQuery", () => {
  const plain = { caseSensitive: false, wholeWord: false, regex: false };

  it("hands the term over unchanged for a literal search", () => {
    expect(seedQuery("scroll.bug (x)", plain)).toBe("scroll.bug (x)");
  });

  it("escapes the term when the saved toggles have regex on", () => {
    // Global find matched it literally; unescaped, `.` and `(` would not.
    const q = seedQuery("scroll.bug (x)", { ...plain, regex: true });
    expect(new RegExp(q).test("scroll.bug (x)")).toBe(true);
    expect(new RegExp(q).test("scrollXbug (x)")).toBe(false);
  });
});

describe("activeIndexAfterSearch", () => {
  const matches = [at(1), at(4), at(9)];

  it("a requested start turn wins, even on a fresh query", () => {
    expect(activeIndexAfterSearch(0, matches, true, 9)).toBe(2);
  });

  it("a fresh query or toggle change starts from the first match", () => {
    expect(activeIndexAfterSearch(2, matches, true, null)).toBe(0);
  });

  it("otherwise keeps your place, clamped to what is left", () => {
    // Live tail or a compact toggle re-runs the search without changing it.
    expect(activeIndexAfterSearch(1, matches, false, null)).toBe(1);
    expect(activeIndexAfterSearch(7, matches, false, null)).toBe(2);
  });

  it("is 0 with nothing matched", () => {
    expect(activeIndexAfterSearch(3, [], false, null)).toBe(0);
  });
});
