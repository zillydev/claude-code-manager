import { useEffect, useMemo, useState } from "react";
import TagChip from "./TagChip";
import { useSessionMeta } from "../hooks/useSessionMeta";
import { entryTags, nextPaletteColor, tagColor } from "../lib/sessionMeta";

/** Edit a session's tags: current tags as removable chips + an input that
 *  autocompletes registry tags; a new name is created with the next palette
 *  color. Centered modal, Esc/backdrop close. */
export default function TagEditor({
  sessionId,
  title,
  onClose,
}: {
  sessionId: string;
  title: string;
  onClose: () => void;
}) {
  const { meta, setSessionTags, upsertTag } = useSessionMeta();
  const [input, setInput] = useState("");
  const current = entryTags(meta.sessions[sessionId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const suggestions = useMemo(() => {
    const q = input.trim().toLowerCase();
    return meta.tags
      .filter((t) => !current.includes(t.name))
      .filter((t) => !q || t.name.toLowerCase().includes(q))
      .slice(0, 8);
  }, [meta.tags, current, input]);

  const addTag = (name: string) => {
    const n = name.trim();
    if (!n || current.includes(n)) return;
    if (!meta.tags.some((t) => t.name === n)) {
      upsertTag(n, nextPaletteColor(meta));
    }
    setSessionTags(sessionId, [...current, n]);
    setInput("");
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
            Tags — {title}
          </span>
          <button
            onClick={onClose}
            className="text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
          >
            ×
          </button>
        </div>

        <div className="flex flex-wrap gap-1 min-h-6">
          {current.length === 0 && (
            <span className="text-xs text-[var(--color-text-dim)]">No tags yet</span>
          )}
          {current.map((name) => (
            <TagChip
              key={name}
              name={name}
              color={tagColor(meta, name)}
              onRemove={() =>
                setSessionTags(
                  sessionId,
                  current.filter((t) => t !== name),
                )
              }
            />
          ))}
        </div>

        <input
          autoFocus
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addTag(input);
            }
          }}
          placeholder="Add a tag (Enter creates a new one)…"
          className="w-full bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-2 py-1.5 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
        />

        {suggestions.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {suggestions.map((t) => (
              <button key={t.name} onClick={() => addTag(t.name)} title="Add this tag">
                <TagChip name={t.name} color={t.color} />
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
