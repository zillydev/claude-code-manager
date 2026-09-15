import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { RenderedTurn } from "../types";
import { activeIndexAfterSearch } from "../lib/findHandoff";
import { compileMatcher, type FindOpts } from "../lib/findMatcher";
import { compareMatches, modelMatches, revealKey, sameMatches, type FindMatch } from "../lib/findMatches";
import {
  clearHighlights,
  domMatches,
  gutterMarks,
  paintHighlights,
  rerasterize,
  scrollMatchIntoView,
  type GutterMark,
} from "../lib/findPaint";
import { usePersistentState } from "./usePersistentState";

export interface FindOptions extends FindOpts {
  /** Include collapsed tool calls / results / task results (auto-expanding on navigation). */
  tools: boolean;
}

const DEFAULT_OPTS: FindOptions = { caseSensitive: false, wholeWord: false, regex: false, tools: false };
const EMPTY_KEYS: ReadonlySet<string> = new Set();

export interface UseFind {
  open: boolean;
  /** Open (or refocus) the bar. `prefill` non-empty replaces the query; `startTurn`
   *  starts on the first match in that turn instead of the first overall. */
  openBar: (prefill?: string | null, startTurn?: number) => void;
  close: () => void;
  query: string;
  setQuery: (s: string) => void;
  opts: FindOptions;
  setOpts: (o: FindOptions) => void;
  count: number;
  activeIndex: number;
  error: string | null;
  next: () => void;
  prev: () => void;
  /** Jump straight to a 0-based match index (clamped into range). */
  goTo: (index: number) => void;
  /** Bumps on every openBar() so the bar can focus+select its input. */
  focusNonce: number;
  /** Collapsibles forced open for navigation — see revealKey(). */
  revealedKeys: ReadonlySet<string>;
  /** Overview-ruler data: each match's 0..1 position in the scroller. */
  marks: GutterMark[];
}

/**
 * In-session find state. `turns` MUST be the array the transcript renders
 * (compact-filtered) and MUST be referentially stable across renders
 * (useMemo) — the recompute effect keys on it. Visible text is matched off the
 * DOM inside `containerRef`; collapsed tool bodies off the model when
 * `opts.tools` is on. Painting happens in a layout effect AFTER any reveal
 * has committed, so an auto-expanded body is highlighted the frame it appears.
 */
export function useFind(containerRef: React.RefObject<HTMLElement | null>, turns: RenderedTurn[]): UseFind {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  // Zed-style: the toggles are a user preference, shared across sessions/windows.
  const [opts, setOpts] = usePersistentState<FindOptions>("find.opts", DEFAULT_OPTS);
  const [matches, setMatches] = useState<FindMatch[]>([]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [revealedKeys, setRevealedKeys] = useState<ReadonlySet<string>>(EMPTY_KEYS);
  const [focusNonce, setFocusNonce] = useState(0);
  const [marks, setMarks] = useState<GutterMark[]>([]);

  const deferredQuery = useDeferredValue(query);
  // Whether the paint effect last left highlights on screen; clearing them needs a re-raster too.
  const painted = useRef(false);
  // Read by the recompute without keying it on every keystroke.
  const queryRef = useRef(query);
  queryRef.current = query;
  // A start-turn request, applied once to the search that `openBar` began.
  const [startAt, setStartAt] = useState<{ turn: number; n: number } | null>(null);
  const appliedStart = useRef(0);
  const lastSearch = useRef<{ query: string; opts: FindOptions } | null>(null);

  const openBar = useCallback((prefill?: string | null, startTurn?: number) => {
    setOpen(true);
    if (prefill) setQuery(prefill);
    if (startTurn !== undefined) setStartAt((prev) => ({ turn: startTurn, n: (prev?.n ?? 0) + 1 }));
    setFocusNonce((n) => n + 1);
  }, []);

  // Close keeps the query (reopen shows the last search, selected) but drops
  // everything derived: matches, error, reveals, paint.
  const close = useCallback(() => {
    setOpen(false);
    setMatches((prev) => (prev.length === 0 ? prev : []));
    setError(null);
    setRevealedKeys((prev) => (prev.size === 0 ? prev : EMPTY_KEYS));
    setMarks((prev) => (prev.length === 0 ? prev : []));
    clearHighlights();
  }, []);

  // Switching the tools toggle off collapses what navigation had opened.
  useEffect(() => {
    if (!opts.tools) setRevealedKeys((prev) => (prev.size === 0 ? prev : EMPTY_KEYS));
  }, [opts.tools]);

  // Recompute matches. Layout effect so the DOM for `turns` has committed.
  // Functional setters that bail when unchanged: this keys on `turns`, and an
  // unconditional fresh array here would loop render → effect → setState.
  useLayoutEffect(() => {
    if (!open || deferredQuery === "") {
      setMatches((prev) => (prev.length === 0 ? prev : []));
      setError(null);
      clearHighlights();
      return;
    }
    const m = compileMatcher(deferredQuery, opts);
    if (!m.ok) {
      setError(m.error);
      setMatches((prev) => (prev.length === 0 ? prev : []));
      clearHighlights();
      return;
    }
    setError(null);
    const container = containerRef.current;
    const dom = container ? domMatches(container, m.find) : [];
    const model = opts.tools ? modelMatches(turns, m.find) : [];
    const all = [...dom, ...model].sort(compareMatches);
    setMatches((prev) => (sameMatches(prev, all) ? prev : all));
    const fresh = lastSearch.current?.query !== deferredQuery || lastSearch.current?.opts !== opts;
    lastSearch.current = { query: deferredQuery, opts };
    // Decided here because a passive effect runs after this one and would reset a requested start;
    // the query check skips a pass that still sees the deferred, pre-prefill query.
    let startTurn: number | null = null;
    if (startAt && startAt.n !== appliedStart.current && deferredQuery === queryRef.current) {
      appliedStart.current = startAt.n;
      startTurn = startAt.turn;
    }
    setActiveIndex((i) => activeIndexAfterSearch(i, all, fresh, startTurn));
  }, [open, deferredQuery, opts, turns, containerRef, startAt]);

  // Stepping onto a match inside a collapsed body opens that body.
  useEffect(() => {
    const key = matches[activeIndex] ? revealKey(matches[activeIndex]) : null;
    if (!key) return;
    setRevealedKeys((prev) => (prev.has(key) ? prev : new Set(prev).add(key)));
  }, [matches, activeIndex]);

  // Paint + scroll — after the reveal above has committed (keyed on it).
  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!open || !container || matches.length === 0) {
      clearHighlights();
      setMarks((prev) => (prev.length === 0 ? prev : []));
      if (!painted.current || !container) return;
      painted.current = false;
      return rerasterize(container);
    }
    const activeRange = paintHighlights(container, matches, activeIndex);
    if (activeRange) scrollMatchIntoView(activeRange);
    painted.current = true;
    const cancelReraster = rerasterize(container);
    const next = gutterMarks(container, matches, activeIndex);
    setMarks((prev) =>
      prev.length === next.length &&
      prev.every((m, i) => m.top === next[i].top && m.active === next[i].active)
        ? prev
        : next,
    );
    return cancelReraster;
  }, [open, matches, activeIndex, revealedKeys, containerRef]);

  const next = useCallback(() => {
    setActiveIndex((i) => (matches.length === 0 ? 0 : (i + 1) % matches.length));
  }, [matches.length]);
  const prev = useCallback(() => {
    setActiveIndex((i) => (matches.length === 0 ? 0 : (i - 1 + matches.length) % matches.length));
  }, [matches.length]);
  const goTo = useCallback(
    (index: number) => {
      setActiveIndex(matches.length === 0 ? 0 : Math.max(0, Math.min(index, matches.length - 1)));
    },
    [matches.length],
  );

  // ⌘G / ⌘⇧G step matches while the bar is open, wherever focus is.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey || e.key.toLowerCase() !== "g") return;
      e.preventDefault();
      e.stopPropagation();
      if (e.shiftKey) prev();
      else next();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, next, prev]);

  return {
    open,
    openBar,
    close,
    query,
    setQuery,
    opts,
    setOpts,
    count: matches.length,
    activeIndex,
    error,
    next,
    prev,
    goTo,
    focusNonce,
    revealedKeys,
    marks,
  };
}
