// The Explanatory output style writes "insights" as an inline-code box:
//   `★ Insight ─────`
//   point text
//   `─────`
// CommonMark collapses those single line breaks into one paragraph and styles the
// backtick rules as inline-code pills — so we detect the block in the raw text and
// render it as a terminal-style callout instead (see InsightBlock in Transcript).

export type MdSeg = { kind: "md"; text: string } | { kind: "insight"; content: string };

// Opening `★ Insight …` rule line, the points, then a closing `─…` rule line.
const INSIGHT_RE = /^`★ Insight[^\n`]*`\n([\s\S]*?)\n`─[^\n`]*`$/gm;

/** Split assistant markdown into plain-markdown runs and insight callouts. Text
 *  with no insight block returns a single `md` segment (unchanged rendering). */
export function splitInsights(text: string): MdSeg[] {
  const segs: MdSeg[] = [];
  let last = 0;
  for (const m of text.matchAll(INSIGHT_RE)) {
    const start = m.index ?? 0;
    if (start > last) segs.push({ kind: "md", text: text.slice(last, start) });
    segs.push({ kind: "insight", content: m[1].trim() });
    last = start + m[0].length;
  }
  if (last < text.length) segs.push({ kind: "md", text: text.slice(last) });
  return segs;
}
