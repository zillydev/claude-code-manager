import { describe, it, expect } from "vitest";
import {
  DEFAULT_META,
  TAG_PALETTE,
  tagColor,
  entryTags,
  matchesTagFilter,
  setAliasIn,
  setNoteIn,
  setSessionTagsIn,
  upsertTagIn,
  removeTagIn,
  nextPaletteColor,
  type SessionMeta,
} from "./sessionMeta";

const base: SessionMeta = {
  version: 1,
  tags: [{ name: "wip", color: "#e5534b" }],
  sessions: { abc: { alias: "auth", tags: ["wip"], note: "n" } },
};

describe("tagColor", () => {
  it("resolves a registered tag's color", () => {
    expect(tagColor(base, "wip")).toBe("#e5534b");
  });
  it("falls back to neutral for an unknown tag", () => {
    expect(tagColor(base, "ghost")).toBe("#8b949e");
  });
});

describe("setAliasIn", () => {
  it("sets a trimmed alias", () => {
    const m = setAliasIn(DEFAULT_META, "x", "  hello  ");
    expect(m.sessions["x"].alias).toBe("hello");
  });
  it("clearing the alias prunes an otherwise-empty entry", () => {
    const m = setAliasIn(DEFAULT_META, "x", "hi");
    const m2 = setAliasIn(m, "x", "");
    expect(m2.sessions["x"]).toBeUndefined();
  });
  it("clearing the alias keeps an entry that still has tags", () => {
    const m = setSessionTagsIn(setAliasIn(DEFAULT_META, "x", "hi"), "x", ["wip"]);
    const m2 = setAliasIn(m, "x", null);
    expect(m2.sessions["x"]).toEqual({ tags: ["wip"] });
  });
  it("does not mutate the input", () => {
    const m = setAliasIn(base, "abc", "new");
    expect(base.sessions["abc"].alias).toBe("auth");
    expect(m.sessions["abc"].alias).toBe("new");
  });
});

describe("setNoteIn", () => {
  it("sets and clears a note (empty prunes when nothing else)", () => {
    const m = setNoteIn(DEFAULT_META, "x", "remember this");
    expect(m.sessions["x"].note).toBe("remember this");
    expect(setNoteIn(m, "x", "").sessions["x"]).toBeUndefined();
  });
});

describe("setSessionTagsIn", () => {
  it("sets tags and empty list prunes", () => {
    const m = setSessionTagsIn(DEFAULT_META, "x", ["a", "b"]);
    expect(entryTags(m.sessions["x"])).toEqual(["a", "b"]);
    expect(setSessionTagsIn(m, "x", []).sessions["x"]).toBeUndefined();
  });
});

describe("upsertTagIn", () => {
  it("creates a new tag", () => {
    const m = upsertTagIn(DEFAULT_META, "api", "#539bf5");
    expect(m.tags).toEqual([{ name: "api", color: "#539bf5" }]);
  });
  it("recolors an existing tag without duplicating", () => {
    const m = upsertTagIn(base, "wip", "#57ab5a");
    expect(m.tags).toEqual([{ name: "wip", color: "#57ab5a" }]);
  });
});

describe("removeTagIn", () => {
  it("removes from the registry AND from every session, pruning empties", () => {
    const m = removeTagIn(base, "wip");
    expect(m.tags).toEqual([]);
    // abc still has alias + note, so it survives without the tag.
    expect(m.sessions["abc"]).toEqual({ alias: "auth", note: "n" });
    const only: SessionMeta = {
      version: 1,
      tags: [{ name: "wip", color: "#e5534b" }],
      sessions: { z: { tags: ["wip"] } },
    };
    expect(removeTagIn(only, "wip").sessions["z"]).toBeUndefined();
  });
});

describe("nextPaletteColor", () => {
  it("picks the first palette color when registry is empty", () => {
    expect(nextPaletteColor(DEFAULT_META)).toBe(TAG_PALETTE[0]);
  });
  it("picks the least-used palette color", () => {
    const m: SessionMeta = {
      version: 1,
      tags: [
        { name: "a", color: TAG_PALETTE[0] },
        { name: "b", color: TAG_PALETTE[1] },
      ],
      sessions: {},
    };
    expect(nextPaletteColor(m)).toBe(TAG_PALETTE[2]);
  });
});

describe("matchesTagFilter", () => {
  it("empty selection matches everything", () => {
    expect(matchesTagFilter(undefined, new Set())).toBe(true);
  });
  it("matches when the entry has ANY selected tag", () => {
    expect(matchesTagFilter({ tags: ["wip", "api"] }, new Set(["api"]))).toBe(true);
    expect(matchesTagFilter({ tags: ["wip"] }, new Set(["api"]))).toBe(false);
    expect(matchesTagFilter(undefined, new Set(["api"]))).toBe(false);
  });
});
