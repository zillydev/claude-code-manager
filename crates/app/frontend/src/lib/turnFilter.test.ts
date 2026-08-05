import { describe, it, expect } from "vitest";
import { filterTurns } from "./turnFilter";
import type { RenderedTurn } from "../types";

const user = (u: string): RenderedTurn =>
  ({ type: "user", uuid: u, text: u }) as unknown as RenderedTurn;
const cmd = (): RenderedTurn => ({ type: "command", command: "/x" }) as unknown as RenderedTurn;
const asst = (u: string, blocks: { type: string }[] = []): RenderedTurn =>
  ({ type: "assistant", uuid: u, blocks }) as unknown as RenderedTurn;
const compaction = (): RenderedTurn =>
  ({ type: "compaction", messages_before: 1 }) as unknown as RenderedTurn;

const kinds = (ts: RenderedTurn[]) => ts.map((t) => t.type);

const turns = [
  user("u1"),
  asst("a1", [{ type: "text" }]),
  asst("a2", [{ type: "tool_call" }]),
  compaction(),
  cmd(),
  asst("a3", [{ type: "thinking" }, { type: "tool_call" }]),
];

describe("filterTurns", () => {
  it("'all' returns the SAME array (memo identity)", () => {
    expect(filterTurns(turns, "all", new Set())).toBe(turns);
  });

  it("'user' keeps user + command turns", () => {
    expect(kinds(filterTurns(turns, "user", new Set()))).toEqual([
      "user", "compaction", "command",
    ]);
  });

  it("'assistant' keeps assistant turns only", () => {
    expect(kinds(filterTurns(turns, "assistant", new Set()))).toEqual([
      "assistant", "assistant", "compaction", "assistant",
    ]);
  });

  it("'tools' keeps assistant turns with a tool_call block", () => {
    const got = filterTurns(turns, "tools", new Set());
    expect(got.map((t) => (t.type === "assistant" ? t.uuid : t.type))).toEqual([
      "a2", "compaction", "a3",
    ]);
  });

  it("'bookmarked' intersects with the bookmark set", () => {
    const got = filterTurns(turns, "bookmarked", new Set(["u1", "a3"]));
    expect(got.map((t) => (t.type === "compaction" ? "compaction" : (t as { uuid: string }).uuid)))
      .toEqual(["u1", "compaction", "a3"]);
  });

  it("compaction dividers always survive", () => {
    expect(kinds(filterTurns(turns, "bookmarked", new Set()))).toEqual(["compaction"]);
  });
});
