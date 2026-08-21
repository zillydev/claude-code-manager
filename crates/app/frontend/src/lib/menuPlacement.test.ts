import { describe, it, expect } from "vitest";
import { placeMenu } from "./menuPlacement";

const view = { width: 1000, height: 800 };
const menu = { width: 200, height: 300 };

describe("placeMenu", () => {
  it("opens down-right of the pointer when there is room", () => {
    expect(placeMenu(100, 100, menu, view)).toEqual({ left: 100, top: 100 });
  });

  it("flips left near the right edge, so the menu never covers the cursor", () => {
    expect(placeMenu(950, 100, menu, view).left).toBe(750);
  });

  it("flips up near the bottom edge", () => {
    expect(placeMenu(100, 780, menu, view).top).toBe(480);
  });

  it("flips both ways in the bottom-right corner", () => {
    expect(placeMenu(980, 790, menu, view)).toEqual({ left: 780, top: 490 });
  });

  it("clamps into view when the menu is bigger than the space", () => {
    const huge = { width: 400, height: 900 };
    const p = placeMenu(10, 10, huge, view);
    expect(p.top).toBeGreaterThanOrEqual(6);
    expect(p.left).toBeGreaterThanOrEqual(6);
  });

  it("never places the menu off the top-left", () => {
    expect(placeMenu(0, 0, menu, view)).toEqual({ left: 6, top: 6 });
  });
});
