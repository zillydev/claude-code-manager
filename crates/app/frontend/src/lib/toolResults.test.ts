import { describe, it, expect } from "vitest";
import { patchToolResults, type ToolResultUpdate } from "./toolResults";
import type { RenderedTurn } from "../types";

const toolCall = (id: string, result: { text: string; is_error: boolean } | null) => ({
  type: "tool_call" as const,
  id,
  name: "Bash",
  input_summary: "cmd",
  input_full: {},
  result,
});

type AssistantTurn = Extract<RenderedTurn, { type: "assistant" }>;

const asstTurn = (blocks: AssistantTurn["blocks"]): RenderedTurn => ({
  type: "assistant",
  timestamp: null,
  model: "claude",
  blocks,
  uuid: "u1",
  edit_index: 0,
  fork_index: 0,
  edit_uuid: "u1",
  fork_uuid: "u1",
  uuids: ["u1"],
  editable: true,
});

const up = (id: string, text = "out"): ToolResultUpdate => ({
  tool_use_id: id,
  text,
  is_error: false,
});

describe("patchToolResults", () => {
  it("fills a resultless tool_call matching by id", () => {
    const turns = [asstTurn([toolCall("toolu_slow", null)])];
    const next = patchToolResults(turns, [up("toolu_slow", "== output ==")]);
    const t = next[0];
    if (t.type !== "assistant" || t.blocks[0].type !== "tool_call") throw new Error("shape");
    expect(t.blocks[0].result).toEqual({ text: "== output ==", is_error: false });
  });

  it("never overwrites an existing result (same-batch pairs already embedded)", () => {
    const turns = [asstTurn([toolCall("toolu_a", { text: "original", is_error: false })])];
    const next = patchToolResults(turns, [up("toolu_a", "late duplicate")]);
    const t = next[0];
    if (t.type !== "assistant" || t.blocks[0].type !== "tool_call") throw new Error("shape");
    expect(t.blocks[0].result?.text).toBe("original");
    expect(next).toBe(turns); // untouched → same reference, no re-render
  });

  it("returns the same array when nothing matches", () => {
    const turns = [asstTurn([toolCall("toolu_x", null)])];
    expect(patchToolResults(turns, [up("toolu_other")])).toBe(turns);
    expect(patchToolResults(turns, [])).toBe(turns);
  });

  it("leaves non-matching turns identical (memo-friendly) and patches only the hit", () => {
    const untouched = asstTurn([toolCall("toolu_done", { text: "ok", is_error: false })]);
    const target = asstTurn([toolCall("toolu_slow", null)]);
    const next = patchToolResults([untouched, target], [up("toolu_slow")]);
    expect(next[0]).toBe(untouched); // same reference → memoized Turn skips
    expect(next[1]).not.toBe(target);
  });
});
