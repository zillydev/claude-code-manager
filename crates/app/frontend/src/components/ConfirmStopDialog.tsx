import { useEffect } from "react";
import { useBackdropDismiss } from "../hooks/useBackdropDismiss";
import type { SessionSummary } from "../types";
import { useSessionMeta } from "../hooks/useSessionMeta";
import { sessionTitle } from "../lib/sessionMeta";
import { stopWording, type StopKind } from "../lib/stopPrompt";

/** Asks before a window close, a quit, or taking a terminal stops sessions Claude is still working in. */
export default function ConfirmStopDialog({
  kind,
  sessions,
  onCancel,
  onConfirm,
}: {
  kind: StopKind;
  sessions: SessionSummary[];
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { meta } = useSessionMeta();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const n = sessions.length;
  const { action, confirm, after } = stopWording(kind);
  const backdrop = useBackdropDismiss(onCancel);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" {...backdrop}>
      <div
        className="w-full max-w-sm rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl space-y-3"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="text-sm font-medium text-[var(--color-text)]">
          Claude is still working in {n === 1 ? "1 session" : `${n} sessions`}
        </div>
        <ul className="text-xs text-[var(--color-text)] space-y-1">
          {sessions.map((s) => (
            <li key={s.id} className="truncate">
              • {sessionTitle(meta, s.id, s.title)}
            </li>
          ))}
        </ul>
        <div className="text-xs text-[var(--color-text-dim)]">
          {action} stops {n === 1 ? "it" : "them"} mid-turn. {after}
        </div>
        <div className="flex justify-end gap-2">
          {/* Cancel takes focus, so a reflexive Enter can't stop the work. */}
          <button
            autoFocus
            onClick={onCancel}
            className="text-xs px-3 py-1 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:text-[var(--color-text)] focus:border-[var(--color-accent)] outline-none"
          >
            Cancel
          </button>
          <button onClick={onConfirm} className="text-xs px-3 py-1 rounded bg-[var(--color-error)] text-white">
            {confirm}
          </button>
        </div>
      </div>
    </div>
  );
}
