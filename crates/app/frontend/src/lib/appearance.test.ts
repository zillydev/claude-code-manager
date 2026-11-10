import { describe, it, expect } from "vitest";
import {
  DEFAULT_MODE,
  MODES,
  SCROLL_PX,
  applyMode,
  boxStyle,
  clampLabel,
  lineCount,
  modeById,
  nextMode,
  toolParts,
  type AppearanceMode,
} from "./appearance";
import type { RenderedBlock, RenderedTurn } from "../types";

type ToolBlock = Extract<RenderedBlock, { type: "tool_call" }>;

const user = (tag: string): RenderedTurn =>
  ({ type: "user", text: tag, uuid: tag, uuids: [tag] }) as unknown as RenderedTurn;
const asst = (tag: string, blocks: RenderedBlock[] = [{ type: "text", text: tag }]): RenderedTurn =>
  ({ type: "assistant", uuid: tag, uuids: [tag], blocks }) as unknown as RenderedTurn;
const tool = (name = "Bash", input: Record<string, unknown> = { command: "ls" }): ToolBlock => ({
  type: "tool_call",
  id: name,
  name,
  input_summary: "",
  input_full: input,
  result: { text: "out", is_error: false },
});
const of = (type: string, extra: Record<string, unknown> = {}): RenderedTurn =>
  ({ type, timestamp: null, ...extra }) as unknown as RenderedTurn;

/** One turn of every type, plus the assistant shapes Conversation treats differently. */
const fixture: RenderedTurn[] = [
  user("q1"),
  asst("text"),
  asst("tool-only", [tool()]),
  asst("narration-only", [{ type: "narration", text: "thinking about it" }]),
  asst("thinking-only", [{ type: "thinking", thinking: "" }]),
  asst("text-and-tool", [{ type: "text", text: "hi" }, tool()]),
  asst("thinking-and-tool", [{ type: "thinking", thinking: "" }, tool()]),
  of("system", { content: "sys" }),
  of("command", { command: "/clear", uuids: [] }),
  of("bash", { command: "ls", stdout: "", stderr: "", uuids: [] }),
  of("summary", { content: "summary" }),
  of("task_notification", { uuid: "t", status: "done", summary: "s", result: "r" }),
  of("api_error", { text: "Prompt is too long" }),
  of("compaction", { messages_before: 3 }),
  user("q2"),
  asst("b1"),
  asst("b2"),
  asst("b3"),
];

const label = (t: RenderedTurn) => (t.type === "user" || t.type === "assistant" ? t.uuid : t.type);

describe("MODES", () => {
  it("holds the five presets in cycle order with unique ids", () => {
    expect(MODES.map((m) => m.id)).toEqual(["default", "conversation", "inputs", "truncated", "everything"]);
    expect(DEFAULT_MODE).toBe(MODES[0]);
  });

  it("gives every preset the same complete key set", () => {
    const keys = (m: AppearanceMode) => Object.keys(m).sort();
    for (const m of MODES) expect(keys(m)).toEqual(keys(DEFAULT_MODE));
  });

  it("is frozen, so a mode's identity can stand for its contents", () => {
    for (const m of MODES) {
      expect(Object.isFrozen(m)).toBe(true);
      expect(Object.isFrozen(m.toolOutput)).toBe(true);
      expect(Object.isFrozen(m.toolOutput.height)).toBe(true);
    }
  });

  // Default is today's transcript; any drift here is a visible change for every user.
  it("pins Default to today's rendering, knob by knob", () => {
    const scroll = { kind: "scroll" };
    expect(DEFAULT_MODE.toolCalls).toBe("shown");
    expect(DEFAULT_MODE.toolInput).toEqual({ show: "collapsed", height: scroll });
    expect(DEFAULT_MODE.toolOutput).toEqual({ show: "collapsed", height: scroll });
    expect(DEFAULT_MODE.diffs).toEqual({ show: "collapsed", height: scroll });
    expect(DEFAULT_MODE.taskNotifications).toEqual({ show: "collapsed", height: scroll });
    expect(DEFAULT_MODE.compactionSummary).toEqual({ show: "open", height: { kind: "full" } });
    expect(DEFAULT_MODE.shellOutput).toEqual(scroll);
    expect(DEFAULT_MODE.narration).toBe("shown");
    expect(DEFAULT_MODE.systemLines).toBe("shown");
    expect(SCROLL_PX).toEqual({
      toolInput: 300,
      toolOutput: 400,
      diffs: 300,
      taskNotifications: 400,
      compactionSummary: 400,
      shellOutput: 400,
    });
  });

  it("sets each preset's levels as designed", () => {
    const clamp = (lines: number) => ({ kind: "clamp", lines });
    const full = { kind: "full" };
    const conversation = modeById("conversation");
    expect([conversation.toolCalls, conversation.narration, conversation.systemLines]).toEqual([
      "hidden",
      "hidden",
      "hidden",
    ]);
    expect(conversation.taskNotifications.show).toBe("hidden");

    const inputs = modeById("inputs");
    expect(inputs.toolInput.show).toBe("open");
    expect(inputs.toolOutput.show).toBe("collapsed");
    expect(inputs.diffs).toEqual({ show: "open", height: { kind: "scroll" } });

    const truncated = modeById("truncated");
    expect(truncated.toolInput).toEqual({ show: "open", height: clamp(12) });
    expect(truncated.toolOutput).toEqual({ show: "open", height: clamp(15) });
    expect(truncated.diffs).toEqual({ show: "open", height: clamp(30) });
    expect(truncated.taskNotifications).toEqual({ show: "open", height: clamp(15) });
    expect(truncated.compactionSummary).toEqual({ show: "open", height: clamp(20) });
    expect(truncated.shellOutput).toEqual(clamp(15));

    const everything = modeById("everything");
    for (const p of [everything.toolInput, everything.toolOutput, everything.diffs, everything.taskNotifications]) {
      expect(p).toEqual({ show: "open", height: full });
    }
    expect(everything.shellOutput).toEqual(full);
  });
});

describe("modeById / nextMode", () => {
  it("maps an unknown id, or a preset that no longer exists, to Default", () => {
    expect(modeById("nope")).toBe(DEFAULT_MODE);
    expect(modeById("skeleton")).toBe(DEFAULT_MODE);
    expect(modeById("truncated").id).toBe("truncated");
  });

  it("cycles forward and wraps", () => {
    expect(nextMode("default").id).toBe("conversation");
    expect(nextMode("everything")).toBe(DEFAULT_MODE);
    expect(nextMode("nope").id).toBe("conversation");
  });
});

describe("applyMode", () => {
  it("returns the same array when the mode drops nothing", () => {
    for (const id of ["default", "inputs", "truncated", "everything"]) {
      expect(applyMode(fixture, modeById(id))).toBe(fixture);
    }
  });

  it("leaves Conversation the prose and keeps the original turn objects", () => {
    const out = applyMode(fixture, modeById("conversation"));
    expect(out.map(label)).toEqual([
      "q1",
      "text",
      "thinking-only",
      "text-and-tool",
      "command",
      "bash",
      "summary",
      "api_error",
      "compaction",
      "q2",
      "b1",
      "b2",
      "b3",
    ]);
    for (const t of out) expect(fixture).toContain(t);
  });
});

describe("toolParts", () => {
  const truncated = modeById("truncated");

  it("puts an Edit's input under diffs", () => {
    const parts = toolParts(truncated, tool("Edit", { file_path: "/a", old_string: "x", new_string: "y" }));
    expect(parts.input).toBe(truncated.diffs);
  });

  it("puts a Bash or JSON input under toolInput", () => {
    expect(toolParts(truncated, tool("Bash")).input).toBe(truncated.toolInput);
    expect(toolParts(truncated, tool("Grep", { pattern: "x" })).input).toBe(truncated.toolInput);
  });

  it("puts the output under toolOutput and file changes under diffs", () => {
    const block = tool();
    expect(toolParts(truncated, block)).toMatchObject({ output: truncated.toolOutput, changes: null });
    const changed: ToolBlock = {
      ...block,
      result: { text: "", is_error: false, file_changes: { files: [], more: 0 } },
    };
    expect(toolParts(truncated, changed).changes).toBe(truncated.diffs);
  });
});

describe("lineCount", () => {
  it("counts newlines plus one, as find sees the text", () => {
    expect(lineCount("")).toBe(1);
    expect(lineCount("a")).toBe(1);
    expect(lineCount("a\nb\n")).toBe(3);
  });
});

describe("boxStyle", () => {
  const PAD = { top: "0.5rem", bottom: "0.5rem" };
  it("keeps today's fixed box for scroll", () => {
    expect(boxStyle({ kind: "scroll" }, 400, PAD)).toEqual({ maxHeight: "400px", overflowY: "auto" });
  });
  it("lets a body that fits keep both paddings", () => {
    expect(boxStyle({ kind: "clamp", lines: 15 }, 400, PAD)).toEqual({
      maxHeight: "calc(15 * 1lh + 0.5rem + 0.5rem)",
      overflow: "hidden",
    });
  });
  it("ends an overflowing clamp on a line boundary, with no part of line N+1 showing", () => {
    expect(boxStyle({ kind: "clamp", lines: 15 }, 400, PAD, true)).toEqual({
      maxHeight: "calc(15 * 1lh + 0.5rem)",
      overflow: "hidden",
      paddingBottom: 0,
    });
  });
  it("sets no height for full", () => {
    expect(boxStyle({ kind: "full" }, 400)).toBeUndefined();
  });
});

describe("clampLabel", () => {
  it("counts the lines when there are more than the clamp shows", () => {
    expect(clampLabel(40, 12, false)).toBe("show all 40 lines");
  });
  it("leaves the count out when wrapping, not line count, is what overflows", () => {
    expect(clampLabel(5, 12, false)).toBe("show all");
    expect(clampLabel(12, 12, false)).toBe("show all");
  });
  it("offers to fold a lifted clamp back", () => {
    expect(clampLabel(40, 12, true)).toBe("show less");
  });
});
