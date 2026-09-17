import { describe, it, expect } from "vitest";
import { focusAtEnd } from "./focusAtEnd";

function field(value: string) {
  const calls: string[] = [];
  const el = {
    value,
    scrollTop: 0,
    scrollHeight: 480,
    focus: () => void calls.push("focus"),
    setSelectionRange: (start: number, end: number) => void calls.push(`select ${start}-${end}`),
  };
  return { el, calls };
}

describe("focusAtEnd", () => {
  it("focuses with the caret after the last character", () => {
    const { el, calls } = field("first line\nsecond line");
    focusAtEnd(el);
    expect(calls).toEqual(["focus", "select 22-22"]);
  });

  it("scrolls a long note so the caret is in view", () => {
    const { el } = field("a\n".repeat(50));
    focusAtEnd(el);
    expect(el.scrollTop).toBe(480);
  });

  it("puts the caret at 0 in an empty note", () => {
    const { el, calls } = field("");
    focusAtEnd(el);
    expect(calls).toEqual(["focus", "select 0-0"]);
  });
});
