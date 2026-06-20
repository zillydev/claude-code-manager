import { describe, it, expect } from "vitest";
import { isNearBottom } from "./scroll";

describe("isNearBottom", () => {
  it("is true when scrolled to the exact bottom", () => {
    expect(isNearBottom({ scrollHeight: 1000, scrollTop: 800, clientHeight: 200 })).toBe(true);
  });

  it("is false when scrolled well up", () => {
    expect(isNearBottom({ scrollHeight: 1000, scrollTop: 0, clientHeight: 200 })).toBe(false);
  });

  it("is true within the default threshold (tolerates a small gap)", () => {
    // distance = 1000 - 750 - 200 = 50 <= 80
    expect(isNearBottom({ scrollHeight: 1000, scrollTop: 750, clientHeight: 200 })).toBe(true);
  });

  it("is false just beyond the default threshold", () => {
    // distance = 1000 - 700 - 200 = 100 > 80
    expect(isNearBottom({ scrollHeight: 1000, scrollTop: 700, clientHeight: 200 })).toBe(false);
  });

  it("respects a custom threshold", () => {
    // distance = 100; threshold 150 → near; threshold 80 → not
    const m = { scrollHeight: 1000, scrollTop: 700, clientHeight: 200 };
    expect(isNearBottom(m, 150)).toBe(true);
    expect(isNearBottom(m, 80)).toBe(false);
  });

  it("treats a non-scrollable (fully visible) container as at-bottom", () => {
    // content shorter than viewport: distance is 0 or negative
    expect(isNearBottom({ scrollHeight: 150, scrollTop: 0, clientHeight: 200 })).toBe(true);
  });
});
