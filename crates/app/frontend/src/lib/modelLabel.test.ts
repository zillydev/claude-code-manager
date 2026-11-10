import { describe, expect, it } from "vitest";
import { effortSummary } from "./modelLabel";

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
