import { describe, it, expect } from "vitest";
import { keptTerminals } from "./terminalViews";

const views = { current: "c", running: "r", exited: "e", other: "o", browsed: "b" };

describe("keptTerminals", () => {
  it("keeps the current session's view and running terminals, dropping the rest", () => {
    const kept = keptTerminals(views, "current", { running: "live", exited: "exited", other: "busy" });
    expect(Object.keys(kept).sort()).toEqual(["current", "running"]);
  });

  it("keeps the current session's view even when its terminal has exited", () => {
    const kept = keptTerminals(views, "exited", { exited: "exited" });
    expect(Object.keys(kept)).toContain("exited");
  });

  it("drops sessions only browsed in Manager, whose terminal never started", () => {
    expect(Object.keys(keptTerminals(views, "current", {}))).toEqual(["current"]);
  });

  it("returns the same object when nothing is dropped, so React skips the update", () => {
    const two = { current: 1, running: 2 };
    expect(keptTerminals(two, "current", { running: "live" })).toBe(two);
  });
});
