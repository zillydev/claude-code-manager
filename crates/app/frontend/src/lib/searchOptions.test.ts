import { describe, it, expect } from "vitest";
import { matchOperators, replaceTrailingToken, suggestTools, trailingToken } from "./searchOptions";

const ops = (token: string) => matchOperators(token).map(([op]) => op);

describe("matchOperators", () => {
  it("offers every operator between tokens", () => {
    expect(ops("")).toContain("tool:");
    expect(ops("")).toHaveLength(9);
  });

  it("narrows to operators the word being typed starts", () => {
    expect(ops("to")).toEqual(["tool:"]);
    expect(ops("IN")).toEqual(["in:"]);
    expect(ops("b")).toEqual(["before:"]);
  });

  it("also matches the fixed value of an operator that has one", () => {
    expect(ops("book")).toEqual(["is:bookmarked"]);
    expect(ops("bo")).toEqual(["is:bookmarked"]);
    expect(ops("link")).toEqual(["has:link"]);
    expect(ops("ha")).toEqual(["has:link"]);
  });

  it("offers nothing for an ordinary word, or once a value is being typed", () => {
    expect(ops("docker")).toEqual([]);
    expect(ops("tool:")).toEqual([]);
    expect(ops("in:mono")).toEqual([]);
  });
});

describe("trailingToken / replaceTrailingToken", () => {
  it("is the word after the last space, empty right after one", () => {
    expect(trailingToken("docker to")).toBe("to");
    expect(trailingToken("docker ")).toBe("");
    expect(trailingToken("")).toBe("");
  });

  it("replaces the partly typed word instead of appending to it", () => {
    expect(replaceTrailingToken("docker to", "tool:")).toBe("docker tool:");
    expect(replaceTrailingToken("docker ", "tool:")).toBe("docker tool:");
    expect(replaceTrailingToken("", "in:")).toBe("in:");
  });
});

describe("suggestTools", () => {
  const tools = [
    { name: "Bash", count: 77030 },
    { name: "Edit", count: 10042 },
    { name: "mcp__tabularis__run_query", count: 7778 },
    { name: "mcp__deployer-prod-mcp__run_mongo_query", count: 940 },
  ];

  it("with nothing typed, lists the most used first", () => {
    expect(suggestTools(tools, "", 2)).toEqual(["Bash", "Edit"]);
  });

  it("matches anywhere in the name, case-insensitively, keeping usage order", () => {
    expect(suggestTools(tools, "RUN")).toEqual(["mcp__tabularis__run_query", "mcp__deployer-prod-mcp__run_mongo_query"]);
  });

  it("drops a name already typed in full, and caps the list", () => {
    expect(suggestTools(tools, "bash")).toEqual([]);
    expect(suggestTools(tools, "")).toHaveLength(4);
    expect(suggestTools([...Array(20)].map((_, i) => ({ name: `T${i}`, count: 1 })), "")).toHaveLength(8);
  });
});
