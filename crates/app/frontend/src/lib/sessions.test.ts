import { describe, it, expect } from "vitest";
import { sessionUpdatedAt } from "./sessions";

const s = (last: number | null, mtime: number) => ({
  last_event_at: last === null ? null : { secs_since_epoch: last, nanos_since_epoch: 0 },
  mtime: { secs_since_epoch: mtime, nanos_since_epoch: 0 },
});

describe("sessionUpdatedAt", () => {
  it("prefers freshest live activity over everything", () => {
    expect(sessionUpdatedAt(s(100, 9999), 200)).toBe(200);
  });
  it("uses the discovered last-event time when no live activity", () => {
    expect(sessionUpdatedAt(s(100, 9999), null)).toBe(100);
    expect(sessionUpdatedAt(s(100, 9999), undefined)).toBe(100);
  });
  it("falls back to file mtime ONLY when the session has no real turn", () => {
    expect(sessionUpdatedAt(s(null, 50), null)).toBe(50);
  });
  it("treats a 0 activity timestamp as a real value, not missing", () => {
    // ?? (not ||) so an epoch of 0 doesn't fall through.
    expect(sessionUpdatedAt(s(100, 50), 0)).toBe(0);
  });
});
