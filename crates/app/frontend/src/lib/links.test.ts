import { describe, it, expect } from "vitest";
import { extractLinks } from "./links";
import type { RenderedTurn } from "../types";

const user = (u: string, text: string): RenderedTurn =>
  ({ type: "user", uuid: u, text }) as unknown as RenderedTurn;
const asst = (u: string, texts: string[]): RenderedTurn =>
  ({
    type: "assistant",
    uuid: u,
    blocks: texts.map((text) => ({ type: "text", text })),
  }) as unknown as RenderedTurn;
const tool = (u: string, result: string): RenderedTurn =>
  ({
    type: "assistant",
    uuid: u,
    blocks: [{ type: "tool_call", id: "t", name: "Fetch", input_summary: "", result: { text: result, is_error: false } }],
  }) as unknown as RenderedTurn;

describe("extractLinks", () => {
  it("finds bare and markdown URLs with attribution", () => {
    const got = extractLinks([
      user("u1", "see https://github.com/a/b/pull/7 please"),
      asst("a1", ["docs: [here](https://docs.rs/tauri) ."]),
    ]);
    expect(got).toEqual([
      { url: "https://github.com/a/b/pull/7", domain: "github.com", uuid: "u1", role: "user" },
      { url: "https://docs.rs/tauri", domain: "docs.rs", uuid: "a1", role: "assistant" },
    ]);
  });

  it("strips trailing sentence punctuation", () => {
    const got = extractLinks([user("u1", "go to https://example.com/x.")]);
    expect(got[0].url).toBe("https://example.com/x");
  });

  it("dedupes by URL, first occurrence wins", () => {
    const got = extractLinks([
      user("u1", "https://example.com"),
      asst("a1", ["again https://example.com"]),
    ]);
    expect(got).toHaveLength(1);
    expect(got[0].uuid).toBe("u1");
  });

  it("does not scan tool results", () => {
    // A fetched page dumps hundreds of URLs; the conversation's own links are
    // the ones that matter.
    expect(extractLinks([tool("a1", "noise https://noise.example/1")])).toEqual([]);
  });

  it("returns empty for a linkless session", () => {
    expect(extractLinks([user("u1", "no links here")])).toEqual([]);
  });
});
