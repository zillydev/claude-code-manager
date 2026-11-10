import { describe, it, expect, afterEach, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { AnsiText } from "./AnsiText";
import { ansiText } from "../lib/ansi";
import { rangeForOffsets } from "../lib/findPaint";

const RAW = "\x1b[2m Test Files \x1b[22m \x1b[1m\x1b[32m9 passed\x1b[39m\x1b[22m\x1b[90m (9)\x1b[39m a<b&c";

function decode(s: string): string {
  return s
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#x27;/g, "'")
    .replace(/&amp;/g, "&");
}

/** The DOM text nodes the markup produces, in order (React's `<!-- -->` separators split adjacent strings). */
function textNodes(html: string): { data: string }[] {
  return html
    .split(/<[^>]*>/)
    .filter((t) => t !== "")
    .map((t) => ({ data: decode(t) }));
}

/** A stand-in element whose tree walker yields `nodes`, so rangeForOffsets runs without a DOM library. */
function fakeEl(nodes: { data: string }[]) {
  const range = { start: null as unknown, startOff: 0, end: null as unknown, endOff: 0 };
  const el = {
    ownerDocument: {
      createTreeWalker: () => {
        let i = 0;
        return { nextNode: () => nodes[i++] ?? null };
      },
      createRange: () => ({
        setStart: (n: unknown, o: number) => Object.assign(range, { start: n, startOff: o }),
        setEnd: (n: unknown, o: number) => Object.assign(range, { end: n, endOff: o }),
      }),
    },
  };
  return { el: el as unknown as Element, range };
}

afterEach(() => vi.unstubAllGlobals());

describe("AnsiText", () => {
  const html = renderToStaticMarkup(<AnsiText text={RAW} />);

  it("draws exactly the text find searches", () => {
    expect(textNodes(html).map((n) => n.data).join("")).toBe(ansiText(RAW));
  });

  it("styles runs inline, inheriting colour where none is set", () => {
    expect(html).toContain('<span style="color:var(--ansi-2);font-weight:600">9 passed</span>');
    expect(html).toContain('<span style="opacity:0.65"> Test Files </span>');
    expect(html).not.toContain("\x1b");
  });

  it("keeps find's offsets on the right characters across a style boundary", () => {
    const shown = ansiText(RAW);
    const start = shown.indexOf("passed (9)");
    const nodes = textNodes(html);
    const { el, range } = fakeEl(nodes);
    vi.stubGlobal("NodeFilter", { SHOW_TEXT: 4 });
    expect(rangeForOffsets(el, start, start + "passed (9)".length)).not.toBeNull();
    expect(range.start).not.toBe(range.end);
    const si = nodes.indexOf(range.start as { data: string });
    const ei = nodes.indexOf(range.end as { data: string });
    const painted =
      nodes[si].data.slice(range.startOff) +
      nodes.slice(si + 1, ei).map((n) => n.data).join("") +
      nodes[ei].data.slice(0, range.endOff);
    expect(painted).toBe("passed (9)");
  });

  it("swaps colours for inverse, falling back to the surface it sits on", () => {
    const inv = renderToStaticMarkup(<AnsiText text={"\x1b[7mx\x1b[31;7my"} />);
    // The outer span's currentColor is still the inherited text colour.
    expect(inv).toContain('<span style="background-color:currentColor"><span style="color:var(--color-tool-result-bg)">x</span></span>');
    expect(inv).toContain("background-color:var(--ansi-1)");
  });
});
