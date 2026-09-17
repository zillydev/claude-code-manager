import { describe, it, expect } from "vitest";
import {
  loadPosition,
  parsePositions,
  pickAnchor,
  POSITIONS_KEY,
  savePosition,
  withoutPosition,
  withPosition,
  type TurnBox,
} from "./scrollMemory";

const turns: TurnBox[] = [
  { uuid: "a", top: 0, bottom: 100 },
  { uuid: "b", top: 110, bottom: 400 },
  { uuid: "c", top: 410, bottom: 500 },
];
const anchorAt = (viewTop: number, list = turns) => pickAnchor(list.length, (i) => list[i], viewTop);

describe("pickAnchor", () => {
  it("picks the turn crossing the top of the view and how far into it the view starts", () => {
    expect(anchorAt(250)).toEqual({ uuid: "b", offset: 140 });
  });

  it("picks a turn starting exactly at the top", () => {
    expect(anchorAt(110)).toEqual({ uuid: "b", offset: 0 });
  });

  it("picks the next turn when the top falls in the gap between two", () => {
    expect(anchorAt(105)).toEqual({ uuid: "b", offset: -5 });
  });

  it("picks the first turn above everything, and nothing below everything", () => {
    expect(anchorAt(-20)).toEqual({ uuid: "a", offset: -20 });
    expect(anchorAt(500)).toBeNull();
    expect(anchorAt(0, [])).toBeNull();
  });

  it("measures only a few turns of a long transcript", () => {
    const long: TurnBox[] = Array.from({ length: 10_000 }, (_, i) => ({ uuid: `t${i}`, top: i * 50, bottom: i * 50 + 40 }));
    let measured = 0;
    const found = pickAnchor(long.length, (i) => (measured++, long[i]), 123_456);
    expect(found).toEqual({ uuid: "t2469", offset: 6 });
    expect(measured).toBeLessThan(20);
  });
});

describe("parsePositions", () => {
  it("reads saved positions and drops malformed entries", () => {
    const raw = JSON.stringify({
      s1: { uuid: "u", offset: 12, at: 5 },
      s2: { uuid: 3, offset: 1, at: 1 },
      s3: "nope",
      s4: { uuid: "u", offset: null, at: 1 },
    });
    expect(parsePositions(raw)).toEqual({ s1: { uuid: "u", offset: 12, at: 5 } });
  });

  it("treats missing or corrupt storage as empty", () => {
    expect(parsePositions(null)).toEqual({});
    expect(parsePositions("{not json")).toEqual({});
    expect(parsePositions("[1,2]")).toEqual({});
  });
});

describe("withPosition / withoutPosition", () => {
  it("sets a session's position", () => {
    expect(withPosition({}, "s", { uuid: "u", offset: 3 }, 9)).toEqual({ s: { uuid: "u", offset: 3, at: 9 } });
  });

  it("drops the least recently saved past the cap", () => {
    let p = withPosition({}, "old", { uuid: "u", offset: 0 }, 1, 2);
    p = withPosition(p, "mid", { uuid: "u", offset: 0 }, 2, 2);
    p = withPosition(p, "new", { uuid: "u", offset: 0 }, 3, 2);
    expect(Object.keys(p).sort()).toEqual(["mid", "new"]);
  });

  it("forgets a position, returning the same object when there was none", () => {
    const p = withPosition({}, "s", { uuid: "u", offset: 0 }, 1);
    expect(withoutPosition(p, "s")).toEqual({});
    expect(withoutPosition(p, "other")).toBe(p);
  });
});

function fakeStorage() {
  const m = new Map<string, string>();
  return {
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => void m.set(k, v),
    raw: () => m.get(POSITIONS_KEY),
  };
}

describe("loadPosition / savePosition", () => {
  it("round-trips a position and forgets it on null", () => {
    const s = fakeStorage();
    savePosition("s", { uuid: "u", offset: 40 }, 1, s);
    expect(loadPosition("s", s)).toEqual({ uuid: "u", offset: 40 });
    savePosition("s", null, 2, s);
    expect(loadPosition("s", s)).toBeNull();
  });

  it("doesn't write when forgetting a session with nothing saved", () => {
    const s = fakeStorage();
    savePosition("s", null, 1, s);
    expect(s.raw()).toBeUndefined();
  });

  it("survives storage that throws or is missing", () => {
    const broken = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("full");
      },
    };
    expect(loadPosition("s", broken)).toBeNull();
    expect(() => savePosition("s", { uuid: "u", offset: 0 }, 1, broken)).not.toThrow();
    expect(loadPosition("s", null)).toBeNull();
  });
});
