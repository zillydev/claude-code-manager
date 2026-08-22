import { describe, it, expect } from "vitest";
import { highlightToLines, languageForPath, type Token } from "./syntax";

const join = (line: Token[]) => line.map((t) => t.text).join("");

describe("languageForPath", () => {
  it("maps the extensions this project actually edits", () => {
    expect(languageForPath("/a/b/main.rs")).toBe("rust");
    expect(languageForPath("src/App.tsx")).toBe("typescript");
    expect(languageForPath("x.YAML")).toBe("yaml");
    expect(languageForPath("Cargo.toml")).toBe("ini");
  });

  it("returns null when it doesn't know, rather than guessing", () => {
    expect(languageForPath("/a/b/file.xyz")).toBeNull();
    expect(languageForPath("noextension")).toBeNull();
    expect(languageForPath(null)).toBeNull();
  });
});

describe("highlightToLines", () => {
  it("preserves the text exactly, line for line", () => {
    const src = "fn main() {\n    let x = 1; // note\n}";
    const lines = highlightToLines(src, "rust");
    expect(lines.map(join)).toEqual(src.split("\n"));
  });

  it("marks tokens with highlight.js classes", () => {
    const [first] = highlightToLines("fn main() {}", "rust");
    expect(first.some((t) => t.cls?.includes("hljs-keyword"))).toBe(true);
  });

  it("keeps multi-line constructs in one class — the point of not highlighting line by line", () => {
    const lines = highlightToLines("/* one\n   two */\ncode", "rust");
    // Both comment lines carry the comment class; the code line doesn't.
    expect(lines[0].every((t) => t.cls?.includes("comment"))).toBe(true);
    expect(lines[1].every((t) => t.cls?.includes("comment"))).toBe(true);
    expect(lines[2].some((t) => t.cls?.includes("comment"))).toBe(false);
  });

  it("falls back to plain text for an unknown or missing language", () => {
    const src = "anything at all\nsecond";
    expect(highlightToLines(src, null).map(join)).toEqual(src.split("\n"));
    expect(highlightToLines(src, "not-a-language").map(join)).toEqual(src.split("\n"));
  });

  it("keeps empty lines as empty lines", () => {
    expect(highlightToLines("a\n\nb", "rust").map(join)).toEqual(["a", "", "b"]);
  });
});
