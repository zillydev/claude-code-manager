import { describe, it, expect } from "vitest";
import { deriveStatus } from "./sessionStatus";

describe("deriveStatus", () => {
  it("running when last event is within the 10s window", () => {
    expect(deriveStatus(1000, 0, 1005).running).toBe(true);
    expect(deriveStatus(1000, 0, 1009).running).toBe(true);
  });
  it("not running at/after the 10s boundary", () => {
    expect(deriveStatus(1000, 0, 1010).running).toBe(false);
    expect(deriveStatus(1000, 0, 1100).running).toBe(false);
  });
  it("unread when last event is newer than last viewed", () => {
    expect(deriveStatus(1000, 999, 9999).unread).toBe(true);
    expect(deriveStatus(1000, 1000, 9999).unread).toBe(false);
    expect(deriveStatus(1000, 1001, 9999).unread).toBe(false);
  });
  it("neither when there are no events", () => {
    expect(deriveStatus(null, 0, 9999)).toEqual({ running: false, unread: false });
  });
});
