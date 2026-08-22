import { describe, it, expect } from "vitest";
import {
  MODEL_FIELDS,
  compareMatches,
  modelMatches,
  parseSegId,
  revealKey,
  sameMatches,
  toolInputText,
  toolResultText,
  type FindMatch,
} from "./findMatches";
import { compileMatcher } from "./findMatcher";
import { makeSegId } from "./segments";
import type { RenderedTurn } from "../types";

function find(q: string) {
  const m = compileMatcher(q, { caseSensitive: false, wholeWord: false, regex: false });
  if (!m.ok) throw new Error();
  return m.find;
}

const turns: RenderedTurn[] = [
  {
    type: "user", timestamp: null, text: "needle in user text", uuid: "u1",
    edit_index: 0, fork_index: 0, edit_uuid: "u1", fork_uuid: "u1", uuids: ["u1"], editable: true,
  },
  {
    type: "assistant", timestamp: null, model: null, uuid: "a1",
    edit_index: 1, fork_index: 1, edit_uuid: "a1", fork_uuid: "a1", uuids: ["a1"], editable: true,
    blocks: [
      { type: "text", text: "needle in markdown" },
      {
        type: "tool_call", id: "t1", name: "Bash", input_summary: "ls",
        input_full: { command: "ls needle" },
        result: { text: '{"k":"needle"}', is_error: false },
      },
    ],
  },
  { type: "task_notification", timestamp: null, uuid: "n1", status: "done", summary: "needle summary", result: "needle result" },
];

describe("modelMatches", () => {
  it("only searches collapsed-body fields, with the renderer's transforms", () => {
    const ms = modelMatches(turns, find("needle"));
    expect(ms.map((m) => m.field)).toEqual(["tool-input", "tool-result", "task-result"]);
    // tool-input is the pretty-printed JSON (Bash isn't a diff tool), so the
    // offset is inside it
    const input = ms[0];
    expect(toolInputText("Bash", { command: "ls needle" }).slice(input.start, input.end)).toBe(
      "needle",
    );
    // tool-result is prettyResultText'd (JSON gets pretty-printed)
    const result = ms[1];
    const pretty = toolResultText({ text: '{"k":"needle"}', is_error: false });
    expect(pretty).toContain("\n"); // it was prettified
    expect(pretty.slice(result.start, result.end)).toBe("needle");
    expect(ms[2]).toMatchObject({ segId: makeSegId(2, null, "task-result"), start: 0, end: 6 });
  });

  it("MODEL_FIELDS names exactly the collapsed-body fields", () => {
    expect([...MODEL_FIELDS].sort()).toEqual(["task-result", "tool-input", "tool-result"]);
  });
});

describe("toolInputText", () => {
  it("renders file-editing tools as the diff the transcript shows", () => {
    const t = toolInputText("Edit", { old_string: "a\nb", new_string: "a\nB", file_path: "/x" });
    expect(t).toBe("  a\n- b\n+ B");
    // The offsets a match reports must land in THAT text, not in the JSON.
    expect(t.slice(t.indexOf("B"), t.indexOf("B") + 1)).toBe("B");
  });

  it("keeps JSON for everything else", () => {
    expect(toolInputText("Bash", { command: "ls" })).toBe(JSON.stringify({ command: "ls" }, null, 2));
  });
});

describe("ordering + keys", () => {
  const mk = (turnIndex: number, blockIndex: number | null, field: FindMatch["field"], start: number): FindMatch => ({
    segId: makeSegId(turnIndex, blockIndex, field), turnIndex, blockIndex, field, start, end: start + 1,
  });

  it("compareMatches sorts by turn, block, field rank, then offset", () => {
    const list = [
      mk(1, 0, "tool-result", 0),
      mk(0, null, "user-text", 5),
      mk(1, 0, "tool-input", 9),
      mk(1, 0, "tool-name", 0),
      mk(0, null, "user-text", 1),
      mk(1, 1, "assistant-text", 0),
    ].sort(compareMatches);
    expect(list.map((m) => `${m.turnIndex}:${m.blockIndex}:${m.field}:${m.start}`)).toEqual([
      "0:null:user-text:1",
      "0:null:user-text:5",
      "1:0:tool-name:0",
      "1:0:tool-input:9",
      "1:0:tool-result:0",
      "1:1:assistant-text:0",
    ]);
  });

  it("parseSegId round-trips makeSegId", () => {
    expect(parseSegId(makeSegId(3, 2, "tool-input"))).toEqual({ turnIndex: 3, blockIndex: 2, field: "tool-input" });
    expect(parseSegId(makeSegId(7, null, "user-text"))).toEqual({ turnIndex: 7, blockIndex: null, field: "user-text" });
  });

  it("revealKey addresses the collapsible that must open for a match", () => {
    expect(revealKey(mk(4, 2, "tool-input", 0))).toBe("4:2");
    expect(revealKey(mk(4, 2, "tool-result", 0))).toBe("4:2");
    expect(revealKey(mk(9, null, "task-result", 0))).toBe("9:task");
    expect(revealKey(mk(9, null, "user-text", 0))).toBeNull();
    expect(revealKey(mk(4, 2, "tool-name", 0))).toBeNull();
  });

  it("sameMatches compares by position, not identity", () => {
    const a = [mk(0, null, "user-text", 1)];
    expect(sameMatches(a, [mk(0, null, "user-text", 1)])).toBe(true);
    expect(sameMatches(a, [mk(0, null, "user-text", 2)])).toBe(false);
    expect(sameMatches(a, [])).toBe(false);
  });
});
