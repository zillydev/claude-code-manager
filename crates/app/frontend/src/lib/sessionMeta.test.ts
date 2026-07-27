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
  addSessionTagIn,
  renameTagIn,
  reconcileMeta,
  nextPaletteColor,
  sessionTitle,
  matchesSessionName,
  tagSuggestions,
  type SessionMeta,
} from "./sessionMeta";

const base: SessionMeta = {
  version: 1,
  tags: [{ name: "wip", color: "#e5534b" }],
  sessions: { abc: { alias: "auth", tags: ["wip"], note: "n" } },
};

describe("sessionTitle", () => {
  it("uses the alias when one is set (what the row shows)", () => {
    expect(sessionTitle(base, "abc", "claude auto name")).toBe("auth");
  });
  it("falls back to claude's title when there is no entry", () => {
    expect(sessionTitle(base, "unknown", "claude auto name")).toBe(
      "claude auto name",
    );
  });
  it("falls back when the entry has no alias", () => {
    const m: SessionMeta = { ...base, sessions: { abc: { tags: ["wip"] } } };
    expect(sessionTitle(m, "abc", "claude auto name")).toBe("claude auto name");
  });
  it("treats a blank/whitespace alias as unset", () => {
    const m: SessionMeta = { ...base, sessions: { abc: { alias: "   " } } };
    expect(sessionTitle(m, "abc", "claude auto name")).toBe("claude auto name");
  });
});

describe("tagSuggestions", () => {
  const many: SessionMeta = {
    version: 1,
    tags: Array.from({ length: 12 }, (_, i) => ({
      name: `tag-${i}`,
      color: TAG_PALETTE[i % TAG_PALETTE.length],
    })),
    sessions: {},
  };

  it("returns EVERY match, with no cap", () => {
    // Regression: a fixed .slice(0, 8) hid the tail of the registry once it grew
    // past 8, so tags existed in the filter but were unreachable in the editor.
    expect(tagSuggestions(many, [], "")).toHaveLength(12);
  });
  it("excludes tags the session already has", () => {
    const s = tagSuggestions(many, ["tag-0", "tag-1"], "");
    expect(s).toHaveLength(10);
    expect(s.map((t) => t.name)).not.toContain("tag-0");
  });
  it("narrows by a case-insensitive query", () => {
    expect(tagSuggestions(base, [], "WI").map((t) => t.name)).toEqual(["wip"]);
    expect(tagSuggestions(base, [], "zzz")).toEqual([]);
  });
});

describe("matchesSessionName", () => {
  // base: sessions.abc has alias "auth"; claude's own title is passed in.
  it("matches the local alias", () => {
    expect(matchesSessionName(base, "abc", "claude auto name", "AUT")).toBe(true);
  });
  it("still matches claude's title when an alias is set", () => {
    // The row shows claude's name dim beneath the alias, so it must stay findable.
    expect(matchesSessionName(base, "abc", "refactor login", "refactor")).toBe(true);
  });
  it("matches claude's title when there is no alias", () => {
    expect(matchesSessionName(base, "nope", "refactor login", "login")).toBe(true);
  });
  it("does not match an unrelated query", () => {
    expect(matchesSessionName(base, "abc", "refactor login", "zzz")).toBe(false);
  });
  it("does not match across the alias/title boundary", () => {
    expect(matchesSessionName(base, "abc", "refactor", "authrefactor")).toBe(false);
  });
  it("an empty query matches everything", () => {
    expect(matchesSessionName(base, "abc", "anything", "   ")).toBe(true);
  });
});

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

describe("addSessionTagIn", () => {
  it("creates the registry entry AND adds to the session in ONE meta", () => {
    // Regression: doing these as two separate writes raced — one half was lost.
    const m = addSessionTagIn(DEFAULT_META, "x", "wip");
    expect(m.tags).toEqual([{ name: "wip", color: TAG_PALETTE[0] }]);
    expect(entryTags(m.sessions["x"])).toEqual(["wip"]);
  });
  it("reuses an existing registry tag (no duplicate, color kept)", () => {
    const m = addSessionTagIn(base, "x", "wip");
    expect(m.tags).toEqual([{ name: "wip", color: "#e5534b" }]);
    expect(entryTags(m.sessions["x"])).toEqual(["wip"]);
  });
  it("heals a dangling tag: session already has it, registry gets created", () => {
    const dangling: SessionMeta = {
      version: 1,
      tags: [],
      sessions: { z: { tags: ["ghost"] } },
    };
    const m = addSessionTagIn(dangling, "z", "ghost");
    expect(m.tags).toEqual([{ name: "ghost", color: TAG_PALETTE[0] }]);
    expect(entryTags(m.sessions["z"])).toEqual(["ghost"]); // no duplicate
  });
  it("ignores empty names", () => {
    expect(addSessionTagIn(base, "x", "   ")).toEqual(base);
  });
});

describe("renameTagIn", () => {
  it("renames the registry entry (keeping color) and updates sessions", () => {
    const m = renameTagIn(base, "wip", "todo");
    expect(m.tags).toEqual([{ name: "todo", color: "#e5534b" }]);
    expect(m.sessions["abc"]).toEqual({ alias: "auth", tags: ["todo"], note: "n" });
  });
  it("merges into an existing target (drops the old entry, dedups sessions)", () => {
    const m: SessionMeta = {
      version: 1,
      tags: [
        { name: "a", color: "#e5534b" },
        { name: "b", color: "#f0883e" },
      ],
      sessions: { s1: { tags: ["a"] }, s2: { tags: ["a", "b"] } },
    };
    const r = renameTagIn(m, "a", "b");
    expect(r.tags).toEqual([{ name: "b", color: "#f0883e" }]);
    expect(entryTags(r.sessions["s1"])).toEqual(["b"]);
    expect(entryTags(r.sessions["s2"])).toEqual(["b"]);
  });
  it("no-ops on empty, unchanged, or unknown name", () => {
    expect(renameTagIn(base, "wip", "  ")).toBe(base);
    expect(renameTagIn(base, "wip", "wip")).toBe(base);
    expect(renameTagIn(base, "ghost", "x")).toBe(base);
  });
});

describe("reconcileMeta", () => {
  it("registers session tags missing from the registry, with palette colors", () => {
    // Legacy data from the old tag-write race: `ghost`/`orphan` sit on sessions
    // but aren't in the registry, so they never appeared in the tag selector.
    const m: SessionMeta = {
      version: 1,
      tags: [{ name: "wip", color: TAG_PALETTE[0] }],
      sessions: { a: { tags: ["wip", "ghost"] }, b: { tags: ["orphan"] } },
    };
    const r = reconcileMeta(m);
    expect(r.tags.map((t) => t.name).sort()).toEqual(["ghost", "orphan", "wip"]);
    expect(r.tags.every((t) => !!t.color)).toBe(true); // every registered tag colored
    expect(r.tags.find((t) => t.name === "wip")!.color).toBe(TAG_PALETTE[0]); // kept
  });
  it("returns the SAME object when nothing is missing (no needless write)", () => {
    const m: SessionMeta = {
      version: 1,
      tags: [{ name: "wip", color: TAG_PALETTE[0] }],
      sessions: { a: { tags: ["wip"] } },
    };
    expect(reconcileMeta(m)).toBe(m);
  });
  it("leaves orphan registry tags (used by no session) alone", () => {
    const m: SessionMeta = {
      version: 1,
      tags: [{ name: "unused", color: TAG_PALETTE[0] }],
      sessions: {},
    };
    expect(reconcileMeta(m)).toBe(m);
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
