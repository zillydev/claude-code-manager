import { useEffect, useRef, useState } from "react";
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
  onGoto: (index: number) => void;
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
  onGoto,
  onClose,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  // While the match-number field is being edited it holds a draft string;
  // null means "not editing" so the field tracks the active match.
  const [indexDraft, setIndexDraft] = useState<string | null>(null);
  const indexValue = indexDraft ?? String(activeIndex + 1);
  const commitIndex = () => {
    if (indexDraft !== null && indexDraft !== "") {
      const n = parseInt(indexDraft, 10);
      if (!Number.isNaN(n)) onGoto(n - 1);
    }
    setIndexDraft(null);
  };

  const toggle = (key: keyof FindOpts) => setOpts({ ...opts, [key]: !opts[key] });
  const toggleBtn = (active: boolean) =>
    `px-1.5 py-0.5 rounded text-xs font-mono border ${
      active
        ? "border-[var(--color-accent)] text-[var(--color-accent)]"
        : "border-[var(--color-border)] text-[var(--color-text-dim)]"
    } hover:border-[var(--color-accent)] transition-colors`;

  const hasMatches = !error && count > 0;

  return (
    <div className="flex items-center gap-2 px-2 py-1.5 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] shadow-lg">
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
      {hasMatches ? (
        <span className="text-xs tabular-nums text-[var(--color-text-dim)] flex items-center gap-1">
          <input
            value={indexValue}
            onFocus={(e) => {
              setIndexDraft(String(activeIndex + 1));
              e.currentTarget.select();
            }}
            onChange={(e) => setIndexDraft(e.target.value.replace(/[^0-9]/g, ""))}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                commitIndex();
              } else if (e.key === "Escape") {
                e.preventDefault();
                setIndexDraft(null);
                onClose();
              }
            }}
            onBlur={commitIndex}
            aria-label="Go to match number"
            title="Type a match number and press Enter to jump there"
            className="w-10 bg-[var(--color-bg)] text-center rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)] px-1 py-0.5 tabular-nums"
          />
          <span>/ {count}</span>
        </span>
      ) : (
        <span
          className={`text-xs tabular-nums min-w-[3.5rem] ${
            error ? "text-[var(--color-error)]" : "text-[var(--color-text-dim)]"
          }`}
        >
          {error ? error : query ? "No results" : ""}
        </span>
      )}
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
