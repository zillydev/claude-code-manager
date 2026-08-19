import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useState } from "react";
import type { RenderedTurn } from "../types";
import { compileMatcher, type FindOpts } from "../lib/findMatcher";
import { compareMatches, modelMatches, revealKey, sameMatches, type FindMatch } from "../lib/findMatches";
import { clearHighlights, domMatches, paintHighlights, scrollMatchIntoView } from "../lib/findPaint";
import { usePersistentState } from "./usePersistentState";

export interface FindOptions extends FindOpts {
  /** Include collapsed tool calls / results / task results (auto-expanding on navigation). */
  tools: boolean;
}

const DEFAULT_OPTS: FindOptions = { caseSensitive: false, wholeWord: false, regex: false, tools: false };
const EMPTY_KEYS: ReadonlySet<string> = new Set();

export interface UseFind {
  open: boolean;
  /** Open (or refocus) the bar. `prefill` non-empty replaces the query. */
  openBar: (prefill?: string | null) => void;
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

  const deferredQuery = useDeferredValue(query);

  const openBar = useCallback((prefill?: string | null) => {
    setOpen(true);
    if (prefill) setQuery(prefill);
    setFocusNonce((n) => n + 1);
  }, []);

  // Close keeps the query (reopen shows the last search, selected) but drops
  // everything derived: matches, error, reveals, paint.
  const close = useCallback(() => {
    setOpen(false);
    setMatches((prev) => (prev.length === 0 ? prev : []));
    setError(null);
    setRevealedKeys((prev) => (prev.size === 0 ? prev : EMPTY_KEYS));
    clearHighlights();
  }, []);

  // A new query or option set starts from the first match.
  useEffect(() => {
    setActiveIndex(0);
  }, [deferredQuery, opts]);

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
    setActiveIndex((i) => (all.length === 0 ? 0 : Math.min(i, all.length - 1)));
  }, [open, deferredQuery, opts, turns, containerRef]);

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
      return;
    }
    const activeRange = paintHighlights(container, matches, activeIndex);
    if (activeRange) scrollMatchIntoView(activeRange);
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
  };
}
