import { describe, expect, it } from "vitest";
import { captureScale, keepInCapture } from "./messageImage";

describe("captureScale", () => {
  it("renders at the display's density, capped at 2x", () => {
    expect(captureScale(1000, 2)).toBe(2);
    expect(captureScale(1000, 1)).toBe(1);
    expect(captureScale(1000, 3)).toBe(2);
  });

  it("scales a tall message down to the canvas height limit", () => {
    expect(captureScale(12000, 2)).toBe(16384 / 12000);
  });

  it("gives up below half scale", () => {
    expect(captureScale(32768, 2)).toBe(0.5);
    expect(captureScale(40000, 2)).toBeNull();
  });
});

describe("keepInCapture", () => {
  it("drops nodes marked data-capture-skip", () => {
    expect(keepInCapture({ dataset: { captureSkip: "" } } as unknown as Node)).toBe(false);
    expect(keepInCapture({ dataset: {} } as unknown as Node)).toBe(true);
  });

  it("keeps text nodes, which have no dataset", () => {
    expect(keepInCapture({ nodeType: 3 } as unknown as Node)).toBe(true);
  });
});
