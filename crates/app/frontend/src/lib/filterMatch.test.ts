import { describe, it, expect } from "vitest";
import { matches, matchProjectGroups, type ProjectGroup } from "./filterMatch";

const groups: ProjectGroup[] = [
  { name: "mono", cwds: ["/Users/z/Dev/Emergent/mono", "/Users/z/work/mono"] },
  { name: "claude-manager", cwds: ["/Users/z/Dev/Personal/claude-manager"] },
  { name: "deployer", cwds: ["/Users/z/Dev/Emergent/mono/deployer"] },
];

describe("matches", () => {
  it("is case-insensitive and trims the query", () => {
    expect(matches("Emergent", "  emer ")).toBe(true);
    expect(matches("Emergent", "gent")).toBe(true);
    expect(matches("Emergent", "xyz")).toBe(false);
  });

  it("an empty query matches everything", () => {
    expect(matches("anything", "")).toBe(true);
    expect(matches("anything", "   ")).toBe(true);
  });
});

describe("matchProjectGroups", () => {
  it("returns every group for an empty query", () => {
    expect(matchProjectGroups(groups, "")).toHaveLength(3);
  });

  it("matches on the project name, keeping all of its directories", () => {
    const out = matchProjectGroups(groups, "mono");
    // "mono" the name, plus "deployer" whose cwd contains mono/
    expect(out.map((g) => g.name)).toEqual(["mono", "deployer"]);
    expect(out[0].cwds).toHaveLength(2);
  });

  it("matches on a directory, narrowing that group to the matching cwds", () => {
    const out = matchProjectGroups(groups, "work");
    expect(out).toHaveLength(1);
    expect(out[0].name).toBe("mono");
    expect(out[0].cwds).toEqual(["/Users/z/work/mono"]);
  });

  it("drops groups that match neither name nor directory", () => {
    expect(matchProjectGroups(groups, "zzz")).toEqual([]);
  });
});
