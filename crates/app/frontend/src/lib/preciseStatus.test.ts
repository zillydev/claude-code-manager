import { describe, it, expect } from "vitest";
import { showsPreciseStatusNotice } from "./preciseStatus";

describe("showsPreciseStatusNotice", () => {
  it("shows after an automatic install until dismissed", () => {
    expect(showsPreciseStatusNotice("auto", false)).toBe(true);
    expect(showsPreciseStatusNotice("auto", true)).toBe(false);
  });

  it("doesn't show for a choice the user made, or before the backend answers", () => {
    for (const c of ["unset", "on", "off"] as const) expect(showsPreciseStatusNotice(c, false)).toBe(false);
    expect(showsPreciseStatusNotice(null, false)).toBe(false);
  });
});
