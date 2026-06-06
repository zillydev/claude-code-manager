import { describe, it, expect } from "vitest";
import { makeSegId, parseSegId, segRank, compareMatches, exactMatches } from "./segments";
import { compileMatcher } from "./search";
import type { RenderedTurn } from "../types";

const find = (q: string) => {
  const m = compileMatcher(q, { caseSensitive: false, wholeWord: false, regex: false });
  if (!m.ok) throw new Error("bad matcher");
  return m.find;
};

describe("seg-id", () => {
  it("round-trips with a null block index", () => {
    expect(parseSegId(makeSegId(3, null, "user-text"))).toEqual({ turnIndex: 3, blockIndex: null, field: "user-text" });
  });
  it("round-trips with a block index", () => {
    expect(parseSegId(makeSegId(5, 2, "tool-input"))).toEqual({ turnIndex: 5, blockIndex: 2, field: "tool-input" });
  });
});

describe("segRank ordering", () => {
  it("orders by turn, then block, then field", () => {
    const a = segRank(1, 0, "tool-name");
    const b = segRank(1, 0, "tool-input");
    const c = segRank(1, 1, "tool-name");
    const d = segRank(2, 0, "user-text");
    expect(a).toBeLessThan(b);
    expect(b).toBeLessThan(c);
    expect(c).toBeLessThan(d);
  });
});

describe("exactMatches", () => {
  const turns: RenderedTurn[] = [
    { type: "user", timestamp: null, text: "find the cat", uuid: "u1", edit_index: 0, fork_index: 0 },
    {
      type: "assistant",
      timestamp: null,
      model: null,
      uuid: "a1",
      edit_index: 1,
      fork_index: 1,
      blocks: [
        { type: "text", text: "a cat appears" }, // markdown — NOT exact-matched
        {
          type: "tool_call",
          id: "t1",
          name: "Cat",
          input_summary: "",
          input_full: { animal: "cat" },
          result: { text: "one cat found", is_error: false },
        },
      ],
    },
  ];

  it("matches user text, tool name, tool input, tool result — but not markdown text", () => {
    const got = exactMatches(turns, find("cat"));
    const fields = got.map((m) => m.field).sort();
    expect(fields).toEqual(["tool-input", "tool-name", "tool-result", "user-text"]);
  });

  it("sorts into document order with compareMatches", () => {
    const got = exactMatches(turns, find("cat")).sort(compareMatches);
    expect(got[0].field).toBe("user-text");
    expect(got[got.length - 1].field).toBe("tool-result");
  });
});
