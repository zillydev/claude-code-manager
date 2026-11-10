import { describe, expect, it } from "vitest";
import { effortSummary, turnModelLabel } from "./modelLabel";

describe("turnModelLabel", () => {
  it("joins model and effort", () => {
    expect(turnModelLabel("claude-opus-5-5", "high")).toBe("opus-5-5 · high");
  });
  it("shows the model alone for lines without effort", () => {
    expect(turnModelLabel("claude-opus-4-7", null)).toBe("opus-4-7");
  });
  it("shows effort alone without a model, and nothing when neither is known", () => {
    expect(turnModelLabel(null, "xhigh")).toBe("xhigh");
    expect(turnModelLabel(null, null)).toBeNull();
  });
});

describe("effortSummary", () => {
  it("names a single effort that covers every turn without a count", () => {
    expect(effortSummary({ model: "m", turn_count: 4, efforts: [{ effort: "medium", turn_count: 4 }] })).toBe("medium");
  });
  it("counts each effort when they are mixed or partial", () => {
    const m = { model: "m", turn_count: 5, efforts: [{ effort: "medium", turn_count: 3 }, { effort: "xhigh", turn_count: 1 }] };
    expect(effortSummary(m)).toBe("medium ×3 · xhigh ×1");
    expect(effortSummary({ model: "m", turn_count: 5, efforts: [{ effort: "high", turn_count: 2 }] })).toBe("high ×2");
  });
  it("is null when no line recorded an effort", () => {
    expect(effortSummary({ model: "m", turn_count: 5, efforts: [] })).toBeNull();
  });
});
