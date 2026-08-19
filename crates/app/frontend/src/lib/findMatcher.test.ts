import { describe, it, expect } from "vitest";
import { compileMatcher, type FindOpts } from "./findMatcher";

function spans(query: string, opts: Partial<FindOpts>, text: string) {
  const m = compileMatcher(query, { caseSensitive: false, wholeWord: false, regex: false, ...opts });
  if (!m.ok) throw new Error("expected ok matcher");
  return m.find(text);
}

describe("compileMatcher", () => {
  it("plain match is case-insensitive by default", () => {
    expect(spans("foo", {}, "FOO foo Foo")).toHaveLength(3);
  });

  it("respects case-sensitive option", () => {
    expect(spans("foo", { caseSensitive: true }, "FOO foo Foo")).toEqual([{ start: 4, end: 7 }]);
  });

  it("escapes regex metacharacters in plain mode", () => {
    expect(spans("a.b", {}, "axb a.b")).toEqual([{ start: 4, end: 7 }]);
  });

  it("whole-word does not match inside larger words", () => {
    const s = spans("cat", { wholeWord: true }, "cat category a cat.");
    expect(s).toHaveLength(2);
    expect(s[0]).toEqual({ start: 0, end: 3 });
  });

  it("regex mode matches a pattern", () => {
    expect(spans("f.o", { regex: true }, "foo fao fxo")).toHaveLength(3);
  });

  it("invalid regex returns an error", () => {
    const m = compileMatcher("(", { caseSensitive: false, wholeWord: false, regex: true });
    expect(m.ok).toBe(false);
    if (!m.ok) expect(m.error.length).toBeGreaterThan(0);
  });

  it("zero-width regex terminates and skips empty matches", () => {
    expect(spans("a*", { regex: true }, "aaa bbb")).toEqual([{ start: 0, end: 3 }]);
  });

  it("empty query yields no matches", () => {
    expect(spans("", {}, "anything")).toEqual([]);
  });

  it("returns matches in ascending order", () => {
    expect(spans("x", {}, "x_x_x").map((m) => m.start)).toEqual([0, 2, 4]);
  });

  it("a matcher can be reused across texts (lastIndex is reset)", () => {
    const m = compileMatcher("ab", { caseSensitive: false, wholeWord: false, regex: false });
    if (!m.ok) throw new Error();
    expect(m.find("ab ab")).toHaveLength(2);
    expect(m.find("ab")).toHaveLength(1);
  });
});
