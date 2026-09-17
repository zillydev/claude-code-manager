import { useEffect, useRef, useState } from "react";
import { useBackdropDismiss } from "../hooks/useBackdropDismiss";
import { Command, CornerDownLeft } from "lucide-react";
import { useSessionMeta } from "../hooks/useSessionMeta";
import { isSaveShortcut } from "../lib/saveShortcut";
import { focusAtEnd } from "../lib/focusAtEnd";

/** Edit a session's free-text note. Save (or ⌘↵) persists (empty clears); Esc/backdrop
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
  // Opens ready to add to the note: autoFocus alone leaves the caret at the start.
  const textRef = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (textRef.current) focusAtEnd(textRef.current);
  }, []);

  const save = () => {
    setNote(sessionId, draft);
    onClose();
  };
  // The listener reads the latest draft through this, so it isn't re-attached on every keystroke.
  const saveRef = useRef(save);
  saveRef.current = save;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      else if (isSaveShortcut(e)) {
        e.preventDefault(); // no newline in the draft
        saveRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const backdrop = useBackdropDismiss(onClose);
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      {...backdrop}
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
          ref={textRef}
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
            title="Save (⌘↵)"
            className="inline-flex items-center gap-1.5 text-xs px-3 py-1 rounded bg-[var(--color-accent)] text-white"
          >
            Save
            {/* Icons, not ⌘↵ text: the two glyphs come from different fonts and don't share a baseline. */}
            <span className="inline-flex items-center opacity-70" aria-hidden>
              <Command size={11} strokeWidth={2} />
              <CornerDownLeft size={11} strokeWidth={2} />
            </span>
          </button>
        </div>
      </div>
    </div>
  );
}
