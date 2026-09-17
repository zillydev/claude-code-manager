import { describe, it, expect } from "vitest";
import { headerAt, leavesSection, pinSelected } from "./filterRows";

const row = (key: string, section?: string, selected = false) => ({ key, section, selected });

describe("headerAt", () => {
  const rows = [row("all"), row("dev/app", "app"), row("work/app", "app"), row("solo")];

  it("heads the first row of a section", () => {
    expect(headerAt(rows, 1)).toBe("app");
  });

  it("doesn't repeat the header over the section's other rows", () => {
    expect(headerAt(rows, 2)).toBeUndefined();
  });

  it("has no header for rows outside any section", () => {
    expect([headerAt(rows, 0), headerAt(rows, 3)]).toEqual([undefined, undefined]);
  });

  it("heads each section when two meet", () => {
    const meeting = [row("dev/app", "app"), row("dev/api", "api")];
    expect([headerAt(meeting, 0), headerAt(meeting, 1)]).toEqual(["app", "api"]);
  });
});

describe("leavesSection", () => {
  const rows = [row("all"), row("dev/app", "app"), row("work/app", "app"), row("solo"), row("other")];

  it("marks the first row after a section, which nothing else separates", () => {
    expect(leavesSection(rows, 3)).toBe(true);
  });

  it("leaves rows inside a section, and those well clear of one, unmarked", () => {
    expect([leavesSection(rows, 0), leavesSection(rows, 1), leavesSection(rows, 2), leavesSection(rows, 4)]).toEqual([
      false,
      false,
      false,
      false,
    ]);
  });

  it("marks a section that follows another, whose own header then draws the line", () => {
    const meeting = [row("dev/app", "app"), row("dev/api", "api")];
    expect(leavesSection(meeting, 1)).toBe(true);
  });
});

describe("pinSelected", () => {
  it("moves the selected row to the top, with a rule under it", () => {
    const rows = [row("all"), row("a"), row("b", undefined, true)];
    expect(pinSelected(rows).map((r) => r.key)).toEqual(["b", "all", "a"]);
    expect(pinSelected(rows)[1]).toMatchObject({ key: "all", divider: true });
  });

  it("leaves the pinned row's section behind, so it doesn't head the whole list", () => {
    // Selecting one project of a name shared by several: the header belongs to the ones still in place.
    const rows = [row("all"), row("dev/app", "app", true), row("work/app", "app")];
    const pinned = pinSelected(rows);
    expect(pinned.map((r) => [r.key, r.section])).toEqual([
      ["dev/app", undefined],
      ["all", undefined],
      ["work/app", "app"],
    ]);
    expect(headerAt(pinned, 2)).toBe("app");
  });

  it("leaves a list whose selection is already on top alone", () => {
    const rows = [row("a", undefined, true), row("b")];
    expect(pinSelected(rows)).toBe(rows);
  });

  it("leaves a list with nothing selected alone", () => {
    const rows = [row("a"), row("b")];
    expect(pinSelected(rows)).toBe(rows);
  });
});
