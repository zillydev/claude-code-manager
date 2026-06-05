import { useEffect, useRef } from "react";
import type { FindOpts } from "../lib/search";

interface Props {
  query: string;
  setQuery: (s: string) => void;
  opts: FindOpts;
  setOpts: (o: FindOpts) => void;
  count: number;
  activeIndex: number;
  error: string | null;
  onNext: () => void;
  onPrev: () => void;
  onClose: () => void;
}

export default function FindBar({
  query,
  setQuery,
  opts,
  setOpts,
  count,
  activeIndex,
  error,
  onNext,
  onPrev,
  onClose,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const toggle = (key: keyof FindOpts) => setOpts({ ...opts, [key]: !opts[key] });
  const toggleBtn = (active: boolean) =>
    `px-1.5 py-0.5 rounded text-xs font-mono border ${
      active
        ? "border-[var(--color-accent)] text-[var(--color-accent)]"
        : "border-[var(--color-border)] text-[var(--color-text-dim)]"
    } hover:border-[var(--color-accent)] transition-colors`;

  const status = error
    ? error
    : count === 0
      ? query
        ? "No results"
        : ""
      : `${activeIndex + 1} / ${count}`;

  return (
    <div className="absolute top-2 right-4 z-10 flex items-center gap-2 px-2 py-1.5 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] shadow-lg">
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
        placeholder="Find"
        spellCheck={false}
        className="bg-[var(--color-bg)] text-[var(--color-text)] text-sm px-2 py-1 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)] w-48"
      />
      <span
        className={`text-xs tabular-nums min-w-[3.5rem] ${
          error ? "text-[var(--color-error)]" : "text-[var(--color-text-dim)]"
        }`}
      >
        {status}
      </span>
      <button onClick={onPrev} className="text-[var(--color-text-dim)] hover:text-[var(--color-text)] px-1" title="Previous (Shift+Enter)">
        ▲
      </button>
      <button onClick={onNext} className="text-[var(--color-text-dim)] hover:text-[var(--color-text)] px-1" title="Next (Enter)">
        ▼
      </button>
      <span className="w-px self-stretch bg-[var(--color-border)] mx-1" />
      <button onClick={() => toggle("caseSensitive")} className={toggleBtn(opts.caseSensitive)} title="Match case">
        Aa
      </button>
      <button onClick={() => toggle("wholeWord")} className={toggleBtn(opts.wholeWord)} title="Whole word">
        \b
      </button>
      <button onClick={() => toggle("regex")} className={toggleBtn(opts.regex)} title="Regex">
        .*
      </button>
      <button onClick={onClose} className="text-[var(--color-text-dim)] hover:text-[var(--color-text)] px-1" title="Close (Esc)">
        ×
      </button>
    </div>
  );
}
