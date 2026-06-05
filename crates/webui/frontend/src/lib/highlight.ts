import { parseSegId, segRank, type FindMatch } from "./segments";
import type { Span } from "./search";

export function isHighlightSupported(): boolean {
  return typeof CSS !== "undefined" && !!(CSS as any).highlights && typeof (window as any).Highlight === "function";
}

/** Map [start, end) char offsets within el's concatenated text to a DOM Range. */
export function rangeForOffsets(el: Element, start: number, end: number): Range | null {
  const doc = el.ownerDocument;
  const walker = doc.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  let pos = 0;
  let startNode: Text | null = null;
  let startOff = 0;
  let endNode: Text | null = null;
  let endOff = 0;
  let node: Node | null;
  while ((node = walker.nextNode())) {
    const t = node as Text;
    const len = t.data.length;
    if (startNode === null && start <= pos + len) {
      startNode = t;
      startOff = start - pos;
    }
    if (startNode !== null && end <= pos + len) {
      endNode = t;
      endOff = end - pos;
      break;
    }
    pos += len;
  }
  if (!startNode || !endNode) return null;
  const r = doc.createRange();
  r.setStart(startNode, startOff);
  r.setEnd(endNode, endOff);
  return r;
}

/** Matches inside assistant markdown segments, read from the live DOM. */
export function domMarkdownMatches(container: HTMLElement, find: (t: string) => Span[]): FindMatch[] {
  const out: FindMatch[] = [];
  container.querySelectorAll<HTMLElement>("[data-seg-id]").forEach((el) => {
    const segId = el.dataset.segId!;
    const p = parseSegId(segId);
    if (p.field !== "assistant-text") return;
    const text = el.textContent ?? "";
    for (const s of find(text)) {
      out.push({
        segId,
        turnIndex: p.turnIndex,
        blockIndex: p.blockIndex,
        field: "assistant-text",
        rank: segRank(p.turnIndex, p.blockIndex, "assistant-text"),
        start: s.start,
        end: s.end,
      });
    }
  });
  return out;
}

/**
 * Paint cm-find (all mounted, non-active matches) + cm-find-active (the active
 * match) using the CSS Custom Highlight API. Returns the active Range for scroll.
 * Matches whose segId element is not mounted (collapsed tool calls) are skipped.
 */
export function paintHighlights(container: HTMLElement, matches: FindMatch[], activeIndex: number): Range | null {
  const byEl = new Map<string, Element>();
  container.querySelectorAll<HTMLElement>("[data-seg-id]").forEach((el) => byEl.set(el.dataset.segId!, el));

  let activeRange: Range | null = null;

  if (isHighlightSupported()) {
    const HighlightCtor = (window as any).Highlight;
    const rest = new HighlightCtor();
    matches.forEach((m, i) => {
      const el = byEl.get(m.segId);
      if (!el) return;
      const r = rangeForOffsets(el, m.start, m.end);
      if (!r) return;
      if (i === activeIndex) activeRange = r;
      else rest.add(r);
    });
    (CSS as any).highlights.set("cm-find", rest);
    if (activeRange) {
      const act = new HighlightCtor(activeRange);
      act.priority = 1;
      (CSS as any).highlights.set("cm-find-active", act);
    } else {
      (CSS as any).highlights.delete("cm-find-active");
    }
  } else {
    // Fallback: no painting; flash the active segment so it's locatable.
    const m = matches[activeIndex];
    const el = m ? byEl.get(m.segId) : undefined;
    if (el && m) {
      activeRange = rangeForOffsets(el, m.start, m.end);
      el.classList.add("cm-flash");
      setTimeout(() => el.classList.remove("cm-flash"), 800);
    }
  }

  return activeRange;
}

export function clearHighlights(): void {
  if (isHighlightSupported()) {
    (CSS as any).highlights.delete("cm-find");
    (CSS as any).highlights.delete("cm-find-active");
  }
}
