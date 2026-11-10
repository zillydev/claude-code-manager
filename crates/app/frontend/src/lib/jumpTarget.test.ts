import { describe, it, expect } from "vitest";
import { locateJump, turnKeys, turnUuids } from "./jumpTarget";
import type { RenderedTurn } from "../types";
import { applyMode, modeById } from "./appearance";

const user = (uuid: string): RenderedTurn =>
  ({
    type: "user",
    text: "hi",
    uuid,
    uuids: [uuid],
    edit_index: 0,
    fork_index: 0,
    edit_uuid: uuid,
    fork_uuid: uuid,
    editable: true,
  }) as RenderedTurn;
const command = (): RenderedTurn => ({ type: "command", command: "/clear" }) as RenderedTurn;

describe("locateJump", () => {
  it("finds a turn that is on screen", () => {
    const turns = [user("a"), user("b")];
    expect(locateJump("b", turns, turns)).toBe("visible");
  });
  it("separates 'the mode is hiding it' from 'not here at all'", () => {
    const all = [user("a"), user("b")];
    expect(locateJump("b", all, [all[0]])).toBe("mode-hidden");
    // A rewound branch stays in the file but off the rendered path.
    expect(locateJump("zz", all, all)).toBe("off-path");
  });
  it("ignores turns that carry no uuids (a command echo, a bash block)", () => {
    expect(() => locateJump("a", [command()], [command()])).not.toThrow();
    expect(locateJump("a", [command()], [command()])).toBe("off-path");
  });
});

describe("locateJump under appearance modes", () => {
  const asst = (uuid: string, blocks: unknown[]): RenderedTurn =>
    ({ type: "assistant", uuid, uuids: [uuid], blocks }) as unknown as RenderedTurn;
  const toolOnly = asst("tool", [
    { type: "tool_call", id: "t", name: "Bash", input_summary: "", input_full: {}, result: null },
  ]);

  it("reports a turn Conversation drops as mode-hidden", () => {
    const all = [user("q"), toolOnly, asst("answer", [{ type: "text", text: "done" }])];
    const visible = applyMode(all, modeById("conversation"));
    expect(locateJump("tool", all, visible)).toBe("mode-hidden");
    expect(locateJump("answer", all, visible)).toBe("visible");
  });
});

describe("turnUuids", () => {
  it("addresses a task notification by its lone uuid", () => {
    // These render as collapsible turns but carry `uuid`, not `uuids` — one
    // heavy session had 180 of them that no search result could reach.
    const task = {
      type: "task_notification",
      uuid: "t1",
      status: "completed",
      summary: "s",
      result: "r",
    } as RenderedTurn;
    expect(turnUuids(task)).toEqual(["t1"]);
    expect(locateJump("t1", [task], [task])).toBe("visible");
  });
  it("gives nothing for turns that carry no uuid at all", () => {
    expect(turnUuids({ type: "summary", timestamp: null, content: "x" } as RenderedTurn)).toEqual([]);
  });
});

describe("turnKeys", () => {
  it("keys a turn by its first uuid, so a turn keeps its key when the list around it shifts", () => {
    expect(turnKeys([user("a"), user("b")])).toEqual(["a", "b"]);
    expect(turnKeys([user("b")])).toEqual(["b"]);
  });
  it("falls back to the index for turns without a uuid, and never repeats a key", () => {
    expect(turnKeys([command(), user("a"), user("a")])).toEqual(["#0", "a", "a#2"]);
  });
});
