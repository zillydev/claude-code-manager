import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useState } from "react";
import type { RenderedTurn } from "../types";
import { compileMatcher, type FindOpts } from "../lib/search";
import { exactMatches, compareMatches, type FindMatch } from "../lib/segments";
import { domMarkdownMatches, paintHighlights, scrollMatchIntoView, clearHighlights } from "../lib/highlight";

const DEFAULT_OPTS: FindOpts = { caseSensitive: false, wholeWord: false, regex: false };

export interface UseFind {
  open: boolean;
  openFind: () => void;
  close: () => void;
  query: string;
  setQuery: (s: string) => void;
  opts: FindOpts;
  setOpts: (o: FindOpts) => void;
  count: number;
  activeIndex: number;
  error: string | null;
  next: () => void;
  prev: () => void;
  goTo: (index: number) => void;
  revealedToolKeys: Set<string>;
}

export function useFind(containerRef: React.RefObject<HTMLElement | null>, turns: RenderedTurn[]): UseFind {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [opts, setOpts] = useState<FindOpts>(DEFAULT_OPTS);
  const [matches, setMatches] = useState<FindMatch[]>([]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [revealedToolKeys, setRevealedToolKeys] = useState<Set<string>>(new Set());

  const deferredQuery = useDeferredValue(query);

  const openFind = useCallback(() => setOpen(true), []);
  const close = useCallback(() => {
    setOpen(false);
    setQuery("");
    setMatches([]);
    setError(null);
    setRevealedToolKeys(new Set());
    clearHighlights();
  }, []);

  // Reset to the first match whenever the query or options change.
  useEffect(() => {
    setActiveIndex(0);
  }, [deferredQuery, opts]);

  // Recompute matches (exact data-model sources + markdown from the DOM).
  useLayoutEffect(() => {
    if (!open || deferredQuery === "") {
      // Functional + bail-if-empty: this effect keys on `turns`, which callers
      // may derive per render; unconditionally setting a fresh [] here turns
      // that into an infinite render loop.
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
    const exact = exactMatches(turns, m.find);
    const md = container ? domMarkdownMatches(container, m.find) : [];
    const all = [...exact, ...md].sort(compareMatches);
    setMatches(all);
    setActiveIndex((i) => (all.length === 0 ? 0 : Math.min(i, all.length - 1)));
  }, [open, deferredQuery, opts, turns, containerRef]);

  // Reveal the active match's collapsed tool call so it can be painted.
  useEffect(() => {
    const active = matches[activeIndex];
    if (!active) return;
    if (active.field === "tool-input" || active.field === "tool-result") {
      const key = `${active.turnIndex}:${active.blockIndex}`;
      setRevealedToolKeys((prev) => (prev.has(key) ? prev : new Set(prev).add(key)));
    } else if (active.field === "task-result") {
      const key = `${active.turnIndex}:task`;
      setRevealedToolKeys((prev) => (prev.has(key) ? prev : new Set(prev).add(key)));
    }
  }, [matches, activeIndex]);

  // Paint + scroll, after any reveal/expand re-render has committed.
  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container || matches.length === 0) {
      clearHighlights();
      return;
    }
    const activeRange = paintHighlights(container, matches, activeIndex);
    if (activeRange) scrollMatchIntoView(activeRange);
  }, [matches, activeIndex, revealedToolKeys, containerRef]);

  const next = useCallback(() => {
    setActiveIndex((i) => (matches.length === 0 ? 0 : (i + 1) % matches.length));
  }, [matches.length]);
  const prev = useCallback(() => {
    setActiveIndex((i) => (matches.length === 0 ? 0 : (i - 1 + matches.length) % matches.length));
  }, [matches.length]);
  // Jump directly to a 0-based match index, clamped into range.
  const goTo = useCallback((index: number) => {
    setActiveIndex(() => (matches.length === 0 ? 0 : Math.max(0, Math.min(index, matches.length - 1))));
  }, [matches.length]);

  return {
    open,
    openFind,
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
    revealedToolKeys,
  };
}
