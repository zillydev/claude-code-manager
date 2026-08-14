import { describe, it, expect } from "vitest";
import { splitHighlight } from "./highlight";

describe("splitHighlight", () => {
  it("marks case-insensitive occurrences, preserving original casing", () => {
    expect(splitHighlight("Rate limit hit: RATE!", "rate")).toEqual([
      { text: "Rate", hit: true },
      { text: " limit hit: ", hit: false },
      { text: "RATE", hit: true },
      { text: "!", hit: false },
    ]);
  });

  it("empty or whitespace term yields one plain segment", () => {
    expect(splitHighlight("abc", "")).toEqual([{ text: "abc", hit: false }]);
    expect(splitHighlight("abc", "  ")).toEqual([{ text: "abc", hit: false }]);
  });

  it("no match yields one plain segment", () => {
    expect(splitHighlight("abc", "zzz")).toEqual([{ text: "abc", hit: false }]);
  });

  it("adjacent matches produce no empty segments", () => {
    expect(splitHighlight("aaaa", "aa")).toEqual([
      { text: "aa", hit: true },
      { text: "aa", hit: true },
    ]);
  });

  it("match at the very end closes cleanly", () => {
    expect(splitHighlight("the needle", "needle")).toEqual([
      { text: "the ", hit: false },
      { text: "needle", hit: true },
    ]);
  });
});
