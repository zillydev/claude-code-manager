import { describe, it, expect } from "vitest";
import { tabOnOpen } from "./sessionTab";

describe("tabOnOpen", () => {
  it("returns to Terminal for a session left there whose claude still runs", () => {
    expect(tabOnOpen("terminal", "manager", true)).toBe("terminal");
  });

  it("falls back to Manager when that terminal has stopped, rather than resuming claude", () => {
    expect(tabOnOpen("terminal", "terminal", false)).toBe("manager");
  });

  it("returns to Manager for a session left there, even with its terminal running", () => {
    expect(tabOnOpen("manager", "terminal", true)).toBe("manager");
  });

  it("keeps the current tab for an unvisited session with a running terminal", () => {
    expect(tabOnOpen(undefined, "terminal", true)).toBe("terminal");
    expect(tabOnOpen(undefined, "manager", true)).toBe("manager");
  });

  it("opens an unvisited session without a running terminal in Manager", () => {
    expect(tabOnOpen(undefined, "terminal", false)).toBe("manager");
  });
});
