import type { CommitResult } from "../types";

export default function CommitResultBanner({
  result,
  onDismiss,
}: {
  result: CommitResult;
  onDismiss: () => void;
}) {
  const cmd = `claude --resume ${result.new_session_id}`;
  return (
    <div className="px-4 py-3 border-b border-[var(--color-accent)] bg-[var(--color-surface)] text-sm">
      <div className="flex items-center justify-between">
        <span className="text-[var(--color-assistant)]">
          ✓ New session created
        </span>
        <button
          onClick={onDismiss}
          className="text-[var(--color-text-dim)] hover:text-[var(--color-text)] text-xs"
        >
          dismiss
        </button>
      </div>
      <div className="mt-1 flex items-center gap-2">
        <code className="text-xs bg-[var(--color-bg)] px-2 py-1 rounded border border-[var(--color-border)] flex-1 overflow-x-auto">
          {cmd}
        </code>
        <button
          onClick={() => navigator.clipboard.writeText(cmd)}
          className="text-xs px-2 py-1 rounded border border-[var(--color-border)] hover:border-[var(--color-accent)]"
        >
          copy
        </button>
      </div>
      {result.repairs.length > 0 && (
        <div className="mt-1 text-xs text-[var(--color-system)]">
          {result.repairs.length} auto-repair(s):{" "}
          {result.repairs.map((r) => r.kind).join(", ")}
        </div>
      )}
    </div>
  );
}
