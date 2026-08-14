/** Segment of a snippet: `hit` marks an occurrence of the search term. */
export interface HighlightSegment {
  text: string;
  hit: boolean;
}

/** Split `text` around case-insensitive occurrences of `term`, preserving the
 *  original casing. Pure, so the Discord-style match highlighting is testable
 *  without DOM. An empty term yields one non-hit segment. */
export function splitHighlight(text: string, term: string): HighlightSegment[] {
  const t = term.trim().toLowerCase();
  if (!t) return [{ text, hit: false }];
  const lower = text.toLowerCase();
  const out: HighlightSegment[] = [];
  let pos = 0;
  for (;;) {
    const i = lower.indexOf(t, pos);
    if (i === -1) break;
    if (i > pos) out.push({ text: text.slice(pos, i), hit: false });
    out.push({ text: text.slice(i, i + t.length), hit: true });
    pos = i + t.length;
  }
  if (pos < text.length || out.length === 0) out.push({ text: text.slice(pos), hit: false });
  return out;
}
