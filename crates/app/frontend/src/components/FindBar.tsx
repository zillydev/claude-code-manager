import { useEffect, useRef } from "react";
import {
  CaseSensitive,
  ChevronLeft,
  ChevronRight,
  Globe,
  Regex,
  WholeWord,
  Wrench,
  X,
} from "lucide-react";
import type { FindOptions } from "../hooks/useFind";

interface Props {
  query: string;
  setQuery: (s: string) => void;
  opts: FindOptions;
  setOpts: (o: FindOptions) => void;
  count: number;
  activeIndex: number;
  error: string | null;
  /** Bumps whenever the bar should (re)take focus and select its text. */
  focusNonce: number;
  onNext: () => void;
  onPrev: () => void;
  onClose: () => void;
  /** ↗ hand the query to the global find pinned to this session. */
  onSearchAll: () => void;
}

const ICON = { size: 14, strokeWidth: 1.75 } as const;

/** Zed-style buffer-search strip: query · Aa/word/regex · tools · ‹ › n/N · ↗ ✕.
 *  Presentational — all state lives in useFind. */
export default function FindBar({
  query,
  setQuery,
  opts,
  setOpts,
  count,
  activeIndex,
  error,
  focusNonce,
  onNext,
  onPrev,
  onClose,
  onSearchAll,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [focusNonce]);

  const toggle = (key: keyof FindOptions) => setOpts({ ...opts, [key]: !opts[key] });
  // Same active/inactive treatment as the sidebar tab strip.
  const toggleCls = (active: boolean) =>
    `flex items-center justify-center rounded px-1.5 py-1 transition-colors ${
      active
        ? "bg-[var(--color-surface-hover)] text-[var(--color-accent)]"
        : "text-[var(--color-text-dim)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-hover)]"
    }`;
  const plainCls =
    "flex items-center justify-center rounded px-1.5 py-1 text-[var(--color-text-dim)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-hover)] transition-colors disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-[var(--color-text-dim)]";
  const sep = <span className="w-px self-stretch bg-[var(--color-border)] mx-1" aria-hidden />;

  return (
    <div className="flex items-center gap-1 px-3 py-1.5 border-b border-[var(--color-border)] bg-[var(--color-surface)] shrink-0">
      <input
        ref={inputRef}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            if (e.shiftKey) onPrev();
            else onNext();
          } else if (e.key === "Escape") {
            e.preventDefault();
            onClose();
          }
        }}
        placeholder="Find in session"
        spellCheck={false}
        aria-label="Find in session"
        className="flex-1 min-w-0 bg-[var(--color-bg)] text-[var(--color-text)] text-sm px-2 py-1 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
      />

      <button
        onClick={() => toggle("caseSensitive")}
        className={toggleCls(opts.caseSensitive)}
        title="Match case"
        aria-pressed={opts.caseSensitive}
      >
        <CaseSensitive {...ICON} />
      </button>
      <button
        onClick={() => toggle("wholeWord")}
        className={toggleCls(opts.wholeWord)}
        title="Whole word"
        aria-pressed={opts.wholeWord}
      >
        <WholeWord {...ICON} />
      </button>
      <button
        onClick={() => toggle("regex")}
        className={toggleCls(opts.regex)}
        title="Regular expression"
        aria-pressed={opts.regex}
      >
        <Regex {...ICON} />
      </button>

      {sep}
      <button
        onClick={() => toggle("tools")}
        className={toggleCls(opts.tools)}
        title="Include tool calls & results (expands them when navigated to)"
        aria-pressed={opts.tools}
      >
        <Wrench {...ICON} />
      </button>

      {sep}
      <button onClick={onPrev} disabled={count === 0} className={plainCls} title="Previous match (⇧Enter · ⌘⇧G)">
        <ChevronLeft {...ICON} />
      </button>
      <button onClick={onNext} disabled={count === 0} className={plainCls} title="Next match (Enter · ⌘G)">
        <ChevronRight {...ICON} />
      </button>
      <span
        className={`text-xs tabular-nums min-w-[3.5rem] text-center ${
          error ? "text-[var(--color-error)]" : "text-[var(--color-text-dim)]"
        }`}
        title={error ?? undefined}
      >
        {error ? "Invalid regex" : count === 0 ? "0/0" : `${activeIndex + 1}/${count}`}
      </span>

      {sep}
      <button
        onClick={onSearchAll}
        className={plainCls}
        title="Search all sessions with this query (global find, pinned to this session)"
      >
        <Globe {...ICON} />
      </button>
      <button onClick={onClose} className={plainCls} title="Close (Esc)">
        <X {...ICON} />
      </button>
    </div>
  );
}
