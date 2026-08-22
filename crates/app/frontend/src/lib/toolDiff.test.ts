import { describe, it, expect } from "vitest";
import { diffLineText, diffLines, diffText, toolDiff } from "./toolDiff";

const render = (lines: ReturnType<typeof diffLines>) => diffText(lines).split("\n");

describe("diffLines", () => {
  it("keeps common lines as context and marks the changed one", () => {
    expect(render(diffLines("a\nb\nc", "a\nB\nc"))).toEqual(["  a", "- b", "+ B", "  c"]);
  });

  it("handles pure insertion and pure deletion", () => {
    expect(render(diffLines("a\nc", "a\nb\nc"))).toEqual(["  a", "+ b", "  c"]);
    expect(render(diffLines("a\nb\nc", "a\nc"))).toEqual(["  a", "- b", "  c"]);
  });

  it("identical text is all context", () => {
    expect(diffLines("x\ny", "x\ny").every((l) => l.kind === "ctx")).toBe(true);
  });

  it("finds the longest common run rather than pairing line-by-line", () => {
    // A naive zip would call every line changed; the LCS keeps b/c/d as context.
    const out = render(diffLines("b\nc\nd", "a\nb\nc\nd\ne"));
    expect(out).toEqual(["+ a", "  b", "  c", "  d", "+ e"]);
  });
});

describe("toolDiff", () => {
  it("Edit diffs old_string against new_string", () => {
    const d = toolDiff("Edit", { old_string: "one\ntwo", new_string: "one\nTWO" })!;
    expect(render(d)).toEqual(["  one", "- two", "+ TWO"]);
  });

  it("Write is all additions — there is no before", () => {
    const d = toolDiff("Write", { content: "l1\nl2" })!;
    expect(d.every((l) => l.kind === "add")).toBe(true);
    expect(render(d)).toEqual(["+ l1", "+ l2"]);
  });

  it("MultiEdit renders one hunk per edit, blank-separated", () => {
    const d = toolDiff("MultiEdit", {
      edits: [
        { old_string: "a", new_string: "A" },
        { old_string: "b", new_string: "B" },
      ],
    })!;
    expect(render(d)).toEqual(["- a", "+ A", "  ", "- b", "+ B"]);
  });

  it("returns null for other tools, so they keep the JSON view", () => {
    expect(toolDiff("Bash", { command: "ls" })).toBeNull();
    expect(toolDiff("Read", { file_path: "/x" })).toBeNull();
  });

  it("returns null for a malformed input rather than rendering nothing", () => {
    expect(toolDiff("Edit", { file_path: "/x" })).toBeNull();
    expect(toolDiff("Edit", { old_string: "a", new_string: 42 })).toBeNull();
    expect(toolDiff("MultiEdit", { edits: [] })).toBeNull();
    expect(toolDiff("MultiEdit", { edits: [{ old_string: "a" }] })).toBeNull();
  });

  it("falls back to whole-block replacement past the LCS size limit", () => {
    const big = Array.from({ length: 1600 }, (_, i) => `line ${i}`).join("\n");
    const d = toolDiff("Edit", { old_string: big, new_string: big + "\nmore" })!;
    expect(d.filter((l) => l.kind === "ctx")).toHaveLength(0);
    expect(d.filter((l) => l.kind === "del")).toHaveLength(1600);
  });
});

describe("diffText / diffLineText", () => {
  it("the whole text is exactly the lines the renderer emits, newline-joined", () => {
    // The renderer prints diffLineText(l) per line plus a newline between them,
    // so this equality is what keeps find-bar offsets aligned with the DOM.
    const lines = diffLines("a\nb", "a\nB");
    expect(diffText(lines)).toBe(lines.map(diffLineText).join("\n"));
  });
});
