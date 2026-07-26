import { describe, it, expect } from "vitest";
import { filterCompact } from "./compactView";
import type { RenderedTurn } from "../types";

const user = (text: string): RenderedTurn =>
  ({ type: "user", text, uuid: text, uuids: [text] }) as unknown as RenderedTurn;
const asst = (tag: string): RenderedTurn =>
  ({ type: "assistant", uuid: tag, uuids: [tag], blocks: [] }) as unknown as RenderedTurn;
const cmd = (command: string): RenderedTurn =>
  ({ type: "command", command }) as unknown as RenderedTurn;
const sys = (content: string): RenderedTurn =>
  ({ type: "system", content }) as unknown as RenderedTurn;
const compaction = (): RenderedTurn =>
  ({ type: "compaction", messages_before: 3 }) as unknown as RenderedTurn;

/** Identify kept turns by their tag/text, in order. */
const tags = (turns: RenderedTurn[]) =>
  turns.map((t) =>
    t.type === "user" ? t.text
    : t.type === "command" ? t.command
    : t.type === "assistant" ? t.uuid
    : t.type,
  );

describe("filterCompact", () => {
  it("keeps the last reply, which has no following prompt to close it", () => {
    // The regression: claude's answer spans four turns (attachment/system lines
    // split it), and the session ends there. Keeping only the first left the
    // substance invisible until the user sent another message.
    const turns = [user("q1"), asst("a1"), asst("a2"), asst("a3"), asst("a4")];
    expect(tags(filterCompact(turns))).toEqual(["q1", "a1", "a4"]);
  });

  it("keeps the first and last turn of an interior reply", () => {
    const turns = [
      user("q1"),
      asst("a1"), asst("a2"), asst("a3"),
      user("q2"),
      asst("b1"), asst("b2"),
    ];
    expect(tags(filterCompact(turns))).toEqual(["q1", "a1", "a3", "q2", "b1", "b2"]);
  });

  it("keeps a single-turn reply exactly once", () => {
    const turns = [user("q1"), asst("a1"), user("q2"), asst("b1")];
    expect(tags(filterCompact(turns))).toEqual(["q1", "a1", "q2", "b1"]);
  });

  it("treats a slash command as a prompt anchor", () => {
    const turns = [cmd("/compact"), asst("a1"), asst("a2"), asst("a3")];
    expect(tags(filterCompact(turns))).toEqual(["/compact", "a1", "a3"]);
  });

  it("does not let system turns split a reply into two runs", () => {
    // A system turn between assistant turns is still ONE reply, so only its
    // first and last survive — not the pieces on either side of the system turn.
    const turns = [user("q1"), asst("a1"), sys("hook"), asst("a2"), asst("a3")];
    expect(tags(filterCompact(turns))).toEqual(["q1", "a1", "a3"]);
  });

  it("always keeps compaction dividers", () => {
    const turns = [user("q1"), asst("a1"), compaction(), user("q2"), asst("b1")];
    expect(tags(filterCompact(turns))).toEqual(["q1", "a1", "compaction", "q2", "b1"]);
  });

  it("keeps a reply that precedes any prompt", () => {
    const turns = [asst("a1"), asst("a2"), user("q1"), asst("b1")];
    expect(tags(filterCompact(turns))).toEqual(["a1", "a2", "q1", "b1"]);
  });

  it("returns an empty list unchanged", () => {
    expect(filterCompact([])).toEqual([]);
  });
});
