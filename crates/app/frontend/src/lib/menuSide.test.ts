import { describe, it, expect } from "vitest";
import { menuSide } from "./menuSide";

// A 600px-tall list, with a row's ⋯ button somewhere inside it.
const bounds = { top: 0, bottom: 600 };
const trigger = (top: number) => ({ top, bottom: top + 20 });

describe("menuSide", () => {
  it("opens downward, uncapped, when the menu fits below the trigger", () => {
    // Capping a menu to its own measured height rounds it down a fraction and draws a scrollbar.
    expect(menuSide(trigger(100), 150, bounds)).toEqual({ up: false, maxHeight: null });
  });

  it("opens upward for a row near the bottom, where the list would cut the menu off", () => {
    expect(menuSide(trigger(520), 150, bounds)).toEqual({ up: true, maxHeight: null });
  });

  it("stays downward for a row near the top, where there is no room above either", () => {
    expect(menuSide(trigger(10), 150, bounds).up).toBe(false);
  });

  it("takes the roomier side and caps the height when the menu fits neither", () => {
    // 420 below the trigger, 160 above: down, scrolling within what's left.
    expect(menuSide(trigger(160), 500, bounds)).toEqual({ up: false, maxHeight: 416 });
  });

  it("caps against the space above when that is the roomier side", () => {
    // 440 above the trigger, 140 below.
    expect(menuSide(trigger(440), 500, bounds)).toEqual({ up: true, maxHeight: 436 });
  });

  it("measures from the clipping element, not from zero", () => {
    const list = { top: 200, bottom: 500 };
    expect(menuSide(trigger(420), 120, list)).toEqual({ up: true, maxHeight: null });
  });
});
