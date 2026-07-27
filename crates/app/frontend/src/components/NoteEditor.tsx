import { useEffect, useState } from "react";
import { useSessionMeta } from "../hooks/useSessionMeta";

/** Edit a session's free-text note. Save persists (empty clears); Esc/backdrop
 *  cancels without saving. */
export default function NoteEditor({
  sessionId,
  title,
  onClose,
}: {
  sessionId: string;
  title: string;
  onClose: () => void;
}) {
  const { meta, setNote } = useSessionMeta();
  const [draft, setDraft] = useState(meta.sessions[sessionId]?.note ?? "");

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const save = () => {
    setNote(sessionId, draft);
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-full max-w-sm rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl space-y-3"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-[var(--color-text)] truncate">
            Note — {title}
          </span>
          <button
            onClick={onClose}
            className="text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
          >
            ×
          </button>
        </div>
        <textarea
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          rows={5}
          placeholder="What is this session about? (empty clears the note)"
          className="w-full resize-y bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-2 py-1.5 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
        />
        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="text-xs px-3 py-1 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
          >
            Cancel
          </button>
          <button
            onClick={save}
            className="text-xs px-3 py-1 rounded bg-[var(--color-accent)] text-white"
          >
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
