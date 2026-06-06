import { describe, it, expect } from "vitest";
import { compileMatcher } from "./search";

function spans(query: string, opts: Partial<{ caseSensitive: boolean; wholeWord: boolean; regex: boolean }>, text: string) {
  const m = compileMatcher(query, { caseSensitive: false, wholeWord: false, regex: false, ...opts });
  if (!m.ok) throw new Error("expected ok matcher");
  return m.find(text);
}

describe("compileMatcher", () => {
  it("plain match is case-insensitive by default", () => {
    expect(spans("foo", {}, "FOO foo Foo")).toHaveLength(3);
  });

  it("respects case-sensitive option", () => {
    const s = spans("foo", { caseSensitive: true }, "FOO foo Foo");
    expect(s).toEqual([{ start: 4, end: 7 }]);
  });

  it("escapes regex metacharacters in plain mode", () => {
    const s = spans("a.b", {}, "axb a.b");
    expect(s).toEqual([{ start: 4, end: 7 }]);
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
  });

  it("zero-width regex terminates and skips empty matches", () => {
    const s = spans("a*", { regex: true }, "aaa bbb");
    expect(s).toEqual([{ start: 0, end: 3 }]);
  });

  it("empty query yields no matches", () => {
    expect(spans("", {}, "anything")).toEqual([]);
  });

  it("returns matches in ascending order", () => {
    const s = spans("x", {}, "x_x_x");
    expect(s.map((m) => m.start)).toEqual([0, 2, 4]);
  });
});
