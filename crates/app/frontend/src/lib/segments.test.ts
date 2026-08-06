import { describe, it, expect } from "vitest";
import { makeSegId } from "./segments";

describe("makeSegId", () => {
  it("encodes turn, block, and field stably", () => {
    expect(makeSegId(3, 1, "tool-result")).toBe("3:1:tool-result");
    expect(makeSegId(0, null, "user-text")).toBe("0:-:user-text");
  });
});
