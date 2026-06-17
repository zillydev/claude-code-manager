import type { ForkRef, SessionSummary } from "../types";

export default function ForkMarker({
  refs,
  onOpen,
}: {
  refs: ForkRef[];
  onOpen: (s: SessionSummary) => void;
}) {
  return (
    <div className="flex items-center gap-3 my-3">
      <div className="flex-1 border-t border-[var(--color-border)]" />
      <div className="flex flex-wrap items-center justify-center gap-2">
        <span className="text-[10px] uppercase tracking-wider text-[var(--color-text-dim)] whitespace-nowrap">
          ⑂ {refs.length} forked from here
        </span>
        {refs.map(({ session, kind }) => (
          <button
            key={session.id}
            onClick={() => onOpen(session)}
            title={`Open ${session.title}`}
            className="max-w-[16rem] truncate text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors"
          >
            {session.title}
            {kind === "copied" ? " (copy)" : ""}
          </button>
        ))}
      </div>
      <div className="flex-1 border-t border-[var(--color-border)]" />
    </div>
  );
}
