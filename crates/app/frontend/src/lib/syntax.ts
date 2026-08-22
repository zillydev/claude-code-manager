import { createLowlight, common } from "lowlight";
import type { Root, RootContent } from "hast";

const lowlight = createLowlight(common);

/** A run of text sharing one highlight class (absent = plain). */
export interface Token {
  text: string;
  cls?: string;
}

/** File extension → highlight.js language. Unknown extensions return null and
 *  the text renders unhighlighted — a wrong guess is worse than none. */
export function languageForPath(path: string | null | undefined): string | null {
  if (!path) return null;
  const ext = path.toLowerCase().split("/").pop()?.split(".").pop() ?? "";
  const map: Record<string, string> = {
    ts: "typescript", tsx: "typescript", mts: "typescript", cts: "typescript",
    js: "javascript", jsx: "javascript", mjs: "javascript", cjs: "javascript",
    rs: "rust", py: "python", rb: "ruby", go: "go", java: "java", kt: "kotlin",
    swift: "swift", c: "c", h: "c", cpp: "cpp", cc: "cpp", hpp: "cpp", cs: "csharp",
    php: "php", lua: "lua", pl: "perl", r: "r", scala: "scala", dart: "dart",
    sh: "bash", bash: "bash", zsh: "bash", fish: "bash",
    json: "json", yaml: "yaml", yml: "yaml", toml: "ini", ini: "ini",
    xml: "xml", html: "xml", svg: "xml", css: "css", scss: "scss", less: "less",
    md: "markdown", markdown: "markdown", sql: "sql", graphql: "graphql",
    dockerfile: "dockerfile", makefile: "makefile", diff: "diff",
  };
  return map[ext] ?? null;
}

/**
 * Highlight `text` and return its tokens split per line.
 *
 * The WHOLE text is highlighted in one pass, not line by line: a block comment
 * or a multi-line string only parses correctly with its context, and a diff
 * hunk is full of both. Splitting afterwards keeps that context while still
 * letting the renderer place each line on its own row.
 *
 * The concatenated tokens of line N equal line N of the input exactly —
 * highlighting must never add or drop a character, or the find bar's offsets
 * (which are computed on the plain text) would land in the wrong place.
 */
export function highlightToLines(text: string, lang: string | null): Token[][] {
  const plain = (): Token[][] => text.split("\n").map((l) => (l ? [{ text: l }] : []));
  if (!lang || !lowlight.registered(lang)) return plain();

  let tree: Root;
  try {
    tree = lowlight.highlight(lang, text);
  } catch {
    return plain(); // a parse failure must never lose the text
  }

  const lines: Token[][] = [[]];
  const push = (value: string, cls?: string) => {
    const parts = value.split("\n");
    parts.forEach((part, i) => {
      if (i > 0) lines.push([]);
      if (part) lines[lines.length - 1].push({ text: part, cls });
    });
  };
  const walk = (nodes: RootContent[], cls?: string) => {
    for (const n of nodes) {
      if (n.type === "text") {
        push(n.value, cls);
      } else if (n.type === "element") {
        const own = n.properties?.className;
        const nested = Array.isArray(own) ? own.join(" ") : typeof own === "string" ? own : undefined;
        walk(n.children as RootContent[], nested ?? cls);
      }
    }
  };
  walk(tree.children as RootContent[]);
  return lines;
}
