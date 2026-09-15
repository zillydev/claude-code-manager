import { describe, it, expect } from "vitest";
import { locateJump, turnUuids } from "./jumpTarget";
import type { RenderedTurn } from "../types";

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
  it("separates 'compact mode is hiding it' from 'not here at all'", () => {
    const all = [user("a"), user("b")];
    expect(locateJump("b", all, [all[0]])).toBe("compact-hidden");
    // A rewound branch stays in the file but off the rendered path.
    expect(locateJump("zz", all, all)).toBe("off-path");
  });
  it("ignores turns that carry no uuids (a command echo, a bash block)", () => {
    expect(() => locateJump("a", [command()], [command()])).not.toThrow();
    expect(locateJump("a", [command()], [command()])).toBe("off-path");
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
