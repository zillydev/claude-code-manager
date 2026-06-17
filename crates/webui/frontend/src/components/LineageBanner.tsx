import type { SessionSummary } from "../types";

interface Props {
  /** The ancestor this session was copied from, if any. */
  copiedFrom: SessionSummary | null;
  /** Sessions that were copied from this one. */
  copies: SessionSummary[];
  /** Shared-message count with the ancestor (for display). */
  overlap: number | null;
  onOpen: (s: SessionSummary) => void;
}

export default function LineageBanner({ copiedFrom, copies, overlap, onOpen }: Props) {
  if (!copiedFrom && copies.length === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-2 px-4 py-1.5 text-xs border-b border-[var(--color-border)] bg-[var(--color-surface)] shrink-0">
      {copiedFrom && (
        <button
          onClick={() => onOpen(copiedFrom)}
          className="inline-flex items-center gap-1 px-2 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors"
          title="Open the session this one's history was copied from"
        >
          ⮌ Copied from {copiedFrom.title}
          {overlap != null && ` — ${overlap} earlier messages live there`}
        </button>
      )}
      {copies.map((c) => (
        <button
          key={c.id}
          onClick={() => onOpen(c)}
          className="inline-flex items-center gap-1 px-2 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors"
          title="Open the session that was copied from this one"
        >
          Also continued as {c.title} →
        </button>
      ))}
    </div>
  );
}
