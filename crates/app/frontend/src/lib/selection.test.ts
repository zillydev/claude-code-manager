import { describe, it, expect } from "vitest";
import { extendTo, nearestSurvivor, noSelection, pruneTo, toggleRow, type Selection } from "./selection";

const ids = (s: Selection) => [...s.ids].sort();
const order = ["a", "b", "c", "d", "e"];

describe("toggleRow", () => {
  it("adds a row and makes it where a ⇧-click extends from", () => {
    const s = toggleRow(noSelection, "b");
    expect([ids(s), s.anchor]).toEqual([["b"], "b"]);
  });

  it("takes a selected row back out", () => {
    const s = toggleRow(toggleRow(toggleRow(noSelection, "b"), "d"), "b");
    expect(ids(s)).toEqual(["d"]);
  });
});

describe("extendTo", () => {
  it("selects every row between the anchor and the clicked one, in list order", () => {
    expect(ids(extendTo(toggleRow(noSelection, "b"), "d", order))).toEqual(["b", "c", "d"]);
  });

  it("works upward as well as down", () => {
    expect(ids(extendTo(toggleRow(noSelection, "d"), "b", order))).toEqual(["b", "c", "d"]);
  });

  it("replaces the range rather than piling ranges up, keeping the anchor", () => {
    const s = extendTo(extendTo(toggleRow(noSelection, "b"), "e", order), "c", order);
    expect([ids(s), s.anchor]).toEqual([["b", "c"], "b"]);
  });

  it("selects just the clicked row when there's nothing to extend from", () => {
    const s = extendTo(noSelection, "c", order);
    expect([ids(s), s.anchor]).toEqual([["c"], "c"]);
  });

  it("starts afresh when the anchor has left the list", () => {
    const s = extendTo(toggleRow(noSelection, "gone"), "c", order);
    expect([ids(s), s.anchor]).toEqual([["c"], "c"]);
  });
});

describe("starting from the open session", () => {
  it("picks the open session along with the first ⌘-clicked row, as Finder keeps the clicked file", () => {
    const s = toggleRow(noSelection, "d", "b");
    expect([ids(s), s.anchor]).toEqual([["b", "d"], "d"]);
  });

  it("just picks the open session when it's the one ⌘-clicked", () => {
    expect(ids(toggleRow(noSelection, "b", "b"))).toEqual(["b"]);
  });

  it("leaves the open session out once a pick is under way", () => {
    expect(ids(toggleRow(toggleRow(noSelection, "a"), "d", "b"))).toEqual(["a", "d"]);
  });

  it("⇧-clicks a range from the open session when nothing is picked yet", () => {
    const s = extendTo(noSelection, "d", order, "b");
    expect([ids(s), s.anchor]).toEqual([["b", "c", "d"], "b"]);
  });
});

describe("pruneTo", () => {
  it("drops rows a filter hid, so nothing out of sight gets deleted", () => {
    const s = pruneTo(extendTo(toggleRow(noSelection, "a"), "c", order), ["a", "c", "e"]);
    expect(ids(s)).toEqual(["a", "c"]);
  });

  it("forgets an anchor that is no longer visible", () => {
    const s = pruneTo(toggleRow(toggleRow(noSelection, "a"), "c"), ["a", "b"]);
    expect([ids(s), s.anchor]).toEqual([["a"], null]);
  });

  it("returns the same selection when every row is still there", () => {
    const s = toggleRow(noSelection, "a");
    expect(pruneTo(s, order)).toBe(s);
  });
});

describe("nearestSurvivor", () => {
  const before = ["a", "b", "c", "d", "e"];

  it("keeps the row a pick started from when it is still there", () => {
    expect(nearestSurvivor(before, "b", ["a", "b", "e"])).toBe("b");
  });

  it("falls back to the row above when the one it started from went", () => {
    expect(nearestSurvivor(before, "c", ["a", "b", "e"])).toBe("b");
  });

  it("looks below when everything above went too", () => {
    expect(nearestSurvivor(before, "c", ["d", "e"])).toBe("d");
  });

  it("has nothing to scroll to when the list is empty", () => {
    expect(nearestSurvivor(before, "c", [])).toBeNull();
  });

  it("has nothing to scroll to for a row the list never held", () => {
    expect(nearestSurvivor(before, "gone", ["a", "b"])).toBeNull();
  });
});
