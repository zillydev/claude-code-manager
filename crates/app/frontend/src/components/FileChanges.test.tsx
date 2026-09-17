import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ChangesBadge, FileChangesBody } from "./FileChanges";
import { EMPH_CLASS } from "./DiffRow";
import { fileChangesText } from "../lib/fileChanges";
import type { FileChanges } from "../types";

/** An element's textContent, from static markup: tags and React's text separators dropped, entities decoded. */
function textOf(html: string): string {
  return html
    .replace(/<[^>]*>/g, "")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#x27;/g, "'")
    .replace(/&amp;/g, "&");
}

const changes: FileChanges = {
  files: [
    {
      path: "/repo/src/main.rs",
      created: false,
      hunks: [
        { old_start: 1, old_lines: 2, new_start: 1, new_lines: 2, lines: [" fn main() {", '-    println!("a < b");', '+    println!("a & b");'] },
        { old_start: 40, old_lines: 1, new_start: 40, new_lines: 2, lines: [" }", "+// done"] },
      ],
    },
    { path: "/repo/notes.txt", created: true, hunks: [{ old_start: 0, old_lines: 0, new_start: 1, new_lines: 1, lines: ["+hello"] }] },
  ],
  more: 1,
};

describe("FileChangesBody", () => {
  it("draws exactly the text find searches, so match offsets land where they paint", () => {
    const html = renderToStaticMarkup(<FileChangesBody changes={changes} segId="0:0:tool-changes" />);
    expect(html).toContain('data-seg-id="0:0:tool-changes"');
    expect(textOf(html)).toBe(fileChangesText(changes));
  });

  it("emphasizes the characters a replaced line changed", () => {
    const html = renderToStaticMarkup(<FileChangesBody changes={changes} segId="s" />);
    expect(EMPH_CLASS.add).not.toBe("");
    expect(html).toMatch(new RegExp(`class="[^"]*${EMPH_CLASS.add.replace(/[[\]().%_]/g, "\\$&")}[^"]*">&amp;</span>`));
    expect(html).toMatch(new RegExp(`class="[^"]*${EMPH_CLASS.del.replace(/[[\]().%_]/g, "\\$&")}[^"]*">&lt;</span>`));
  });

  it("keeps that text exact without files left out", () => {
    const only = { ...changes, more: 0 };
    expect(textOf(renderToStaticMarkup(<FileChangesBody changes={only} segId="s" />))).toBe(fileChangesText(only));
  });
});

describe("ChangesBadge", () => {
  it("counts files and lines", () => {
    expect(textOf(renderToStaticMarkup(<ChangesBadge changes={changes} />))).toBe("3 files +3 −1");
  });
});
