/** In-session find: the DOM half. Matches for visible segments are read from
 *  the live DOM (`textContent`, so they span markdown/code formatting); all
 *  matches are painted with the CSS Custom Highlight API — no DOM mutation,
 *  so React re-renders can't lose or duplicate marks. */
import type { Span } from "./findMatcher";
import { MODEL_FIELDS, parseSegId, type FindMatch } from "./findMatches";

// The Highlight API has no TS lib typings yet in this toolchain.
type HighlightCtor = new (...ranges: Range[]) => { add(r: Range): void; priority: number };
/* eslint-disable @typescript-eslint/no-explicit-any */
const highlightCtor = (): HighlightCtor | null =>
  typeof window !== "undefined" && typeof (window as any).Highlight === "function"
    ? ((window as any).Highlight as HighlightCtor)
    : null;
const registry = (): Map<string, unknown> | null =>
  typeof CSS !== "undefined" && (CSS as any).highlights
    ? ((CSS as any).highlights as Map<string, unknown>)
    : null;
/* eslint-enable @typescript-eslint/no-explicit-any */

export function isHighlightSupported(): boolean {
  return highlightCtor() !== null && registry() !== null;
}

/** Map [start, end) offsets within el's concatenated text nodes to a Range. */
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

/** Matches in every visible segment (`[data-seg-id]` whose field is not a
 *  model-sourced collapsed body), read from the rendered text. */
export function domMatches(container: HTMLElement, find: (t: string) => Span[]): FindMatch[] {
  const out: FindMatch[] = [];
  container.querySelectorAll<HTMLElement>("[data-seg-id]").forEach((el) => {
    const segId = el.dataset.segId!;
    const p = parseSegId(segId);
    if (MODEL_FIELDS.has(p.field)) return;
    const text = el.textContent ?? "";
    for (const s of find(text)) {
      out.push({ segId, turnIndex: p.turnIndex, blockIndex: p.blockIndex, field: p.field, start: s.start, end: s.end });
    }
  });
  return out;
}

/** Paint `cm-find` (every mounted non-active match) and `cm-find-active`.
 *  Matches whose segment is not mounted (a still-collapsed body) are skipped.
 *  Returns the active match's Range when it is mounted, for scrolling. */
export function paintHighlights(container: HTMLElement, matches: FindMatch[], activeIndex: number): Range | null {
  const byId = new Map<string, Element>();
  container.querySelectorAll<HTMLElement>("[data-seg-id]").forEach((el) => byId.set(el.dataset.segId!, el));

  let activeRange: Range | null = null;
  const Ctor = highlightCtor();
  const reg = registry();
  const rest = Ctor ? new Ctor() : null;

  matches.forEach((m, i) => {
    const el = byId.get(m.segId);
    if (!el) return;
    const r = rangeForOffsets(el, m.start, m.end);
    if (!r) return;
    if (i === activeIndex) activeRange = r;
    else rest?.add(r);
  });

  if (Ctor && reg && rest) {
    reg.set("cm-find", rest);
    if (activeRange) {
      const act = new Ctor(activeRange);
      act.priority = 1;
      reg.set("cm-find-active", act);
    } else {
      reg.delete("cm-find-active");
    }
  }
  return activeRange;
}

/** Centre the matched text in every scrollable ancestor, innermost first —
 *  a match inside a tool-result <pre> (its own overflow-y) needs the <pre>
 *  scrolled AND the transcript scrolled. Recomputes the rect after each step. */
export function scrollMatchIntoView(range: Range): void {
  let el: Element | null =
    range.startContainer.nodeType === Node.ELEMENT_NODE
      ? (range.startContainer as Element)
      : range.startContainer.parentElement;
  while (el) {
    const style = getComputedStyle(el);
    const scrollableY = /(auto|scroll|overlay)/.test(style.overflowY) && el.scrollHeight > el.clientHeight;
    if (scrollableY) {
      const matchRect = range.getBoundingClientRect();
      const elRect = el.getBoundingClientRect();
      el.scrollTop += matchRect.top + matchRect.height / 2 - (elRect.top + el.clientHeight / 2);
    }
    el = el.parentElement;
  }
}

/** A match's vertical position for the scrollbar-gutter ruler. `top` is a
 *  0..1 fraction of the scroller's full content height. */
export interface GutterMark {
  top: number;
  active: boolean;
}

/** Where each match sits in the document, as fractions of scrollHeight — the
 *  overview-ruler data. A match whose own segment is unmounted (a collapsed
 *  tool body) is approximated by its visible stand-in: the tool call's header
 *  line / the task summary line, so every counted match gets a mark. */
export function gutterMarks(container: HTMLElement, matches: FindMatch[], activeIndex: number): GutterMark[] {
  const byId = new Map<string, Element>();
  container.querySelectorAll<HTMLElement>("[data-seg-id]").forEach((el) => byId.set(el.dataset.segId!, el));
  const contRect = container.getBoundingClientRect();
  const height = container.scrollHeight || 1;

  const out: GutterMark[] = [];
  matches.forEach((m, i) => {
    let el = byId.get(m.segId);
    if (!el) {
      // Collapsed body: mark the always-visible line of the same collapsible.
      const standIn =
        m.field === "tool-input" || m.field === "tool-result"
          ? `${m.turnIndex}:${m.blockIndex}:tool-name`
          : m.field === "task-result"
            ? `${m.turnIndex}:-:task-summary`
            : null;
      if (standIn) el = byId.get(standIn);
      if (!el) return;
    }
    let top: number;
    const r = rangeForOffsets(el, m.start, m.end);
    if (r && byId.get(m.segId)) {
      top = r.getBoundingClientRect().top;
    } else {
      top = el.getBoundingClientRect().top;
    }
    out.push({
      top: Math.min(1, Math.max(0, (top - contRect.top + container.scrollTop) / height)),
      active: i === activeIndex,
    });
  });
  return out;
}

/** Rebuild the scroller's compositing layer for two frames. WKWebView repaints less than it painted when a
 *  highlight changes, leaving bands of the old colour; only a full re-raster clears them. Returns a cancel. */
export function rerasterize(scroller: HTMLElement): () => void {
  scroller.style.willChange = "transform";
  let raf2 = 0;
  const raf1 = requestAnimationFrame(() => {
    raf2 = requestAnimationFrame(() => (scroller.style.willChange = ""));
  });
  return () => {
    cancelAnimationFrame(raf1);
    cancelAnimationFrame(raf2);
    scroller.style.willChange = "";
  };
}

export function clearHighlights(): void {
  const reg = registry();
  if (!reg) return;
  reg.delete("cm-find");
  reg.delete("cm-find-active");
}
