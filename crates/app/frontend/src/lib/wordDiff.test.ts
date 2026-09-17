import { describe, it, expect } from "vitest";
import { changedRanges, emphasize, wordEmphasis } from "./wordDiff";
import type { DiffLine } from "./toolDiff";

const at = (s: string, r: [number, number]) => s.slice(r[0], r[1]);

describe("changedRanges", () => {
  it("marks only the text appended to a line", () => {
    const a = "Other tools keep the JSON view.";
    const b = "Other tools keep the JSON view. Shell commands too.";
    const r = changedRanges(a, b)!;
    expect(r.a).toEqual([]);
    expect(r.b.map((x) => at(b, x))).toEqual([" Shell commands too."]);
  });

  it("marks a replaced word on both sides", () => {
    const a = 'println!("a < b");';
    const b = 'println!("a & b");';
    const r = changedRanges(a, b)!;
    expect(r.a.map((x) => at(a, x))).toEqual(["<"]);
    expect(r.b.map((x) => at(b, x))).toEqual(["&"]);
  });

  it("merges neighbouring changed words into one range", () => {
    const a = "let total = price * qty;";
    const b = "let total = price + tax;";
    const r = changedRanges(a, b)!;
    expect(r.a.map((x) => at(a, x))).toEqual(["* qty"]);
    expect(r.b.map((x) => at(b, x))).toEqual(["+ tax"]);
  });

  it("treats non-ASCII words and punctuation as their own tokens", () => {
    const a = "résumé — draft";
    const b = "résumé — final";
    const r = changedRanges(a, b)!;
    expect(r.b.map((x) => at(b, x))).toEqual(["final"]);
  });

  it("gives up on lines that share too little", () => {
    expect(changedRanges("import React from 'react';", "export default function App() {")).toBeNull();
  });
});

const line = (kind: DiffLine["kind"], text: string): DiffLine => ({ kind, text });

describe("wordEmphasis", () => {
  it("pairs deleted and added lines in order within a run of changes", () => {
    const lines = [
      line("ctx", "fn main() {"),
      line("del", "  let a = 1;"),
      line("del", "  let b = 2;"),
      line("add", "  let a = 10;"),
      line("add", "  let b = 20;"),
      line("add", "  let c = 30;"),
      line("ctx", "}"),
    ];
    const e = wordEmphasis(lines);
    expect(e[0]).toBeUndefined();
    expect(e[1]!.map((r) => at(lines[1].text, r))).toEqual(["1"]);
    expect(e[3]!.map((r) => at(lines[3].text, r))).toEqual(["10"]);
    expect(e[2]!.map((r) => at(lines[2].text, r))).toEqual(["2"]);
    expect(e[4]!.map((r) => at(lines[4].text, r))).toEqual(["20"]);
    expect(e[5]).toBeUndefined(); // nothing left to pair with
    expect(e[6]).toBeUndefined();
  });

  it("doesn't pair across context lines", () => {
    const lines = [line("del", "a = 1"), line("ctx", "b"), line("add", "a = 2")];
    expect(wordEmphasis(lines)).toEqual([undefined, undefined, undefined]);
  });

  it("leaves a lone addition or deletion unemphasized", () => {
    expect(wordEmphasis([line("add", "new line")])).toEqual([undefined]);
  });
});

describe("emphasize", () => {
  it("splits tokens at range edges and keeps their classes and text", () => {
    const tokens = [{ text: "let ", cls: "kw" }, { text: "total", cls: "var" }, { text: " = 1;" }];
    const parts = emphasize(tokens, [[6, 12]]);
    expect(parts.map((p) => p.text).join("")).toBe("let total = 1;");
    expect(parts).toEqual([
      { text: "let ", cls: "kw", emph: false },
      { text: "to", cls: "var", emph: false },
      { text: "tal", cls: "var", emph: true },
      { text: " = ", emph: true },
      { text: "1;", emph: false },
    ]);
  });

  it("leaves tokens whole when nothing is emphasized", () => {
    expect(emphasize([{ text: "abc", cls: "x" }], [])).toEqual([{ text: "abc", cls: "x", emph: false }]);
  });
});
