import { describe, it, expect } from "vitest";
import { ansiSegments, ansiText } from "./ansi";

const VITEST =
  "\x1b[2m Test Files \x1b[22m \x1b[1m\x1b[32m9 passed\x1b[39m\x1b[22m\x1b[90m (9)\x1b[39m";

describe("ansiSegments", () => {
  it("turns vitest's summary line into styled runs", () => {
    expect(ansiText(VITEST)).toBe(" Test Files  9 passed (9)");
    expect(ansiSegments(VITEST)).toEqual([
      { text: " Test Files ", style: { dim: true } },
      { text: " " },
      { text: "9 passed", style: { bold: true, fg: "var(--ansi-2)" } },
      { text: " (9)", style: { fg: "var(--ansi-8)" } },
    ]);
  });

  it("passes text without escapes or carriage returns through untouched", () => {
    const json = '{\n  "a": [1, 2],\n  "b": "x"\n}';
    expect(ansiSegments(json)).toEqual([{ text: json }]);
    expect(ansiSegments("")).toEqual([{ text: "" }]);
  });

  it("resets: 0 and empty clear everything, 22 clears bold and dim, 39/49 only their colour", () => {
    expect(ansiSegments("\x1b[1;4;31;42ma\x1b[0mb")).toEqual([
      { text: "a", style: { bold: true, underline: true, fg: "var(--ansi-1)", bg: "var(--ansi-2)" } },
      { text: "b" },
    ]);
    expect(ansiSegments("\x1b[1;3ma\x1b[mb")).toEqual([
      { text: "a", style: { bold: true, italic: true } },
      { text: "b" },
    ]);
    expect(ansiSegments("\x1b[1;2;31ma\x1b[22mb")).toEqual([
      { text: "a", style: { bold: true, dim: true, fg: "var(--ansi-1)" } },
      { text: "b", style: { fg: "var(--ansi-1)" } },
    ]);
    expect(ansiSegments("\x1b[31;44ma\x1b[39mb\x1b[49mc")).toEqual([
      { text: "a", style: { fg: "var(--ansi-1)", bg: "var(--ansi-4)" } },
      { text: "b", style: { bg: "var(--ansi-4)" } },
      { text: "c" },
    ]);
    expect(ansiSegments("\x1b[7;9ma\x1b[27;29mb")).toEqual([
      { text: "a", style: { inverse: true, strike: true } },
      { text: "b" },
    ]);
  });

  it("maps bright and extended colours", () => {
    expect(ansiSegments("\x1b[97;101ma")[0].style).toEqual({ fg: "var(--ansi-15)", bg: "var(--ansi-9)" });
    expect(ansiSegments("\x1b[38;5;196ma")[0].style).toEqual({ fg: "#ff0000" });
    expect(ansiSegments("\x1b[38;5;9ma")[0].style).toEqual({ fg: "var(--ansi-9)" });
    expect(ansiSegments("\x1b[38;5;244ma")[0].style).toEqual({ fg: "#808080" });
    expect(ansiSegments("\x1b[38;2;10;20;30ma")[0].style).toEqual({ fg: "rgb(10,20,30)" });
    expect(ansiSegments("\x1b[48;5;21ma")[0].style).toEqual({ bg: "#0000ff" });
    expect(ansiSegments("\x1b[1;38;5;249mx")[0].style).toEqual({ bold: true, fg: "#b2b2b2" });
  });

  it("ignores unknown SGR codes and private-mode sequences", () => {
    expect(ansiSegments("\x1b[5ma")).toEqual([{ text: "a" }]);
    expect(ansiSegments("\x1b[>4;2ma")).toEqual([{ text: "a" }]);
  });

  it("drops every non-SGR sequence from the text", () => {
    expect(ansiText("a\x1b[2Kb\x1b[1Ac\x1b[Gd\x1b(Be\x1b7f\x1b8g\x1b[?25lh")).toBe("abcdefgh");
    expect(ansiText("see \x1b]8;;https://x\x07label\x1b]8;;\x07 here")).toBe("see label here");
    expect(ansiText("st \x1b]8;;https://x\x1b\\label\x1b]8;;\x1b\\")).toBe("st label");
    expect(ansiText("tail\x1b")).toBe("tail");
    expect(ansiText("cut\x1b[3")).toBe("cut");
    expect(ansiText("\x1b(é ok")).toBe("é ok");
    expect(ansiText("\x1b(😀")).toBe("😀");
  });

  it("keeps only the last redraw of a carriage-returned line", () => {
    expect(ansiText("a\r\nb")).toBe("a\nb");
    expect(ansiText("10%\r50%\r100%\ndone")).toBe("100%\ndone");
    // The clear waits for more text on the line, so a trailing \r erases nothing.
    expect(ansiText("abc\r")).toBe("abc");
    expect(ansiText("abc\r\r\ndef")).toBe("abc\ndef");
    // Simplification: a terminal would show "ab345"; the last redraw wins whole.
    expect(ansiText("12345\rab")).toBe("ab");
    expect(ansiSegments("x\n\x1b[32m10%\r\x1b[1m100%\x1b[0m")).toEqual([
      { text: "x\n" },
      { text: "100%", style: { bold: true, fg: "var(--ansi-2)" } },
    ]);
  });

  it("segments always join to ansiText and never carry ESC", () => {
    const samples = [VITEST, "a\x1b[31mb\rc\nd\x1b", "\x1b]8;;u\x07l\x1b]8;;\x07", "p\r\x1b[2Kq", "plain"];
    for (const s of samples) {
      const segs = ansiSegments(s);
      expect(segs.map((x) => x.text).join("")).toBe(ansiText(s));
      expect(ansiText(s)).not.toContain("\x1b");
      expect(segs.every((x) => x.text.length > 0) || segs.length === 1).toBe(true);
    }
  });
});
