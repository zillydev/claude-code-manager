import { describe, it, expect } from "vitest";
import { deriveStatus } from "./sessionStatus";

describe("deriveStatus", () => {
  it("running while mid-turn (not awaiting) and not stale — through thinking pauses", () => {
    // 60s since the last write but still mid-turn → still running (no flicker).
    expect(deriveStatus(1000, false, 0, 1060)).toEqual({ running: true, unread: false });
  });
  it("not running once the turn completes (awaiting), even seconds after a write", () => {
    expect(deriveStatus(1000, true, 0, 1001).running).toBe(false);
  });
  it("not running when a mid-turn session has gone stale (abandoned)", () => {
    expect(deriveStatus(1000, false, 0, 1000 + 181).running).toBe(false);
  });
  it("unread only when a completed turn is newer than last viewed", () => {
    expect(deriveStatus(1000, true, 999, 9999).unread).toBe(true);
    expect(deriveStatus(1000, true, 1000, 9999).unread).toBe(false);
  });
  it("never unread while mid-turn, however far it has advanced", () => {
    expect(deriveStatus(1000, false, 0, 9999).unread).toBe(false);
  });
  it("a hosted session stays running however long a tool call runs", () => {
    // ccm holds the PTY, so it KNOWS the process is alive — the staleness
    // guess (which a long build or CI poll trips) must not apply.
    expect(deriveStatus(1000, false, 0, 1000 + 3600, true).running).toBe(true);
    // …but a finished turn still ends "running" at once, hosted or not.
    expect(deriveStatus(1000, true, 0, 1001, true).running).toBe(false);
  });
  it("neither when there are no events", () => {
    expect(deriveStatus(null, false, 0, 9999)).toEqual({ running: false, unread: false });
  });
});
