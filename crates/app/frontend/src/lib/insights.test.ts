import { describe, it, expect } from "vitest";
import { splitInsights } from "./insights";

describe("splitInsights", () => {
  it("returns one md segment when there is no insight block", () => {
    expect(splitInsights("just **markdown** text")).toEqual([
      { kind: "md", text: "just **markdown** text" },
    ]);
  });

  it("extracts an insight block between the rules, keeping surrounding markdown", () => {
    const text = "before\n\n`★ Insight ─────`\npoint one\npoint two\n`─────`\n\nafter";
    const segs = splitInsights(text);
    expect(segs.map((s) => s.kind)).toEqual(["md", "insight", "md"]);
    expect(segs[0]).toMatchObject({ kind: "md", text: "before\n\n" });
    expect(segs[1]).toMatchObject({ kind: "insight", content: "point one\npoint two" });
    expect(segs[2]).toMatchObject({ kind: "md", text: "\n\nafter" });
  });

  it("handles an insight block that is the entire message", () => {
    const segs = splitInsights("`★ Insight ──`\nonly point\n`──`");
    expect(segs).toEqual([{ kind: "insight", content: "only point" }]);
  });

  it("ignores a malformed block with no closing rule", () => {
    const text = "`★ Insight ──`\npoint with no close";
    expect(splitInsights(text)).toEqual([{ kind: "md", text }]);
  });
});
