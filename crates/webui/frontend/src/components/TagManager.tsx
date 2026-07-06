import { useEffect, useMemo, useState } from "react";
import { useSessionMeta } from "../hooks/useSessionMeta";
import { TAG_PALETTE, nextPaletteColor, type Tag } from "../lib/sessionMeta";

/** Global tag editor: recolor, rename, or delete every tag in the registry, and
 *  create new ones. Centered modal, Esc/backdrop close. Opened from the sidebar
 *  tag-filter dropdown's "Manage tags…" item. */
export default function TagManager({ onClose }: { onClose: () => void }) {
  const { meta, upsertTag, renameTag, removeTag } = useSessionMeta();
  const [newName, setNewName] = useState("");

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // How many sessions use each tag (shown per row).
  const usage = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const e of Object.values(meta.sessions)) {
      for (const t of e.tags ?? []) counts[t] = (counts[t] ?? 0) + 1;
    }
    return counts;
  }, [meta.sessions]);

  const addTag = () => {
    const n = newName.trim();
    if (!n || meta.tags.some((t) => t.name === n)) return;
    upsertTag(n, nextPaletteColor(meta));
    setNewName("");
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-full max-w-sm max-h-[85vh] overflow-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl space-y-3"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-[var(--color-text)]">Manage tags</span>
          <button
            onClick={onClose}
            className="text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
          >
            ×
          </button>
        </div>

        {meta.tags.length === 0 && (
          <div className="text-xs text-[var(--color-text-dim)]">No tags yet</div>
        )}

        <div className="space-y-1.5">
          {meta.tags.map((t) => (
            <TagRow
              key={t.name}
              tag={t}
              count={usage[t.name] ?? 0}
              onRecolor={(c) => upsertTag(t.name, c)}
              onRename={(n) => renameTag(t.name, n)}
              onDelete={() => removeTag(t.name)}
            />
          ))}
        </div>

        <div className="flex gap-1.5 border-t border-[var(--color-border)] pt-3">
          <input
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                addTag();
              }
            }}
            placeholder="New tag name…"
            className="flex-1 bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-2 py-1.5 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
          />
          <button
            onClick={addTag}
            disabled={!newName.trim()}
            className="text-xs px-3 py-1 rounded bg-[var(--color-accent)] text-white disabled:opacity-40"
          >
            Add
          </button>
        </div>
      </div>
    </div>
  );
}

function TagRow({
  tag,
  count,
  onRecolor,
  onRename,
  onDelete,
}: {
  tag: Tag;
  count: number;
  onRecolor: (color: string) => void;
  onRename: (name: string) => void;
  onDelete: () => void;
}) {
  const [name, setName] = useState(tag.name);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [confirming, setConfirming] = useState(false);

  // Re-sync when the tag changes underneath us (e.g. a merge renamed it).
  useEffect(() => setName(tag.name), [tag.name]);

  const commitRename = () => {
    const n = name.trim();
    if (n && n !== tag.name) onRename(n);
    else setName(tag.name); // revert an empty/unchanged edit
  };

  return (
    <div className="flex items-center gap-2">
      <div className="relative shrink-0">
        <button
          onClick={() => setPaletteOpen((o) => !o)}
          style={{ background: tag.color }}
          className="h-4 w-4 rounded-full border border-[var(--color-border)]"
          title="Change color"
        />
        {paletteOpen && (
          <div className="absolute left-0 top-full z-10 mt-1 flex gap-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-1 shadow-lg">
            {TAG_PALETTE.map((c) => (
              <button
                key={c}
                onClick={() => {
                  onRecolor(c);
                  setPaletteOpen(false);
                }}
                style={{ background: c }}
                className="h-4 w-4 rounded-full border border-[var(--color-border)]"
                title={c}
              />
            ))}
          </div>
        )}
      </div>

      <input
        value={name}
        onChange={(e) => setName(e.target.value)}
        onBlur={commitRename}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            (e.target as HTMLInputElement).blur();
          } else if (e.key === "Escape") {
            setName(tag.name);
            (e.target as HTMLInputElement).blur();
          }
        }}
        className="flex-1 min-w-0 bg-transparent text-xs text-[var(--color-text)] px-1 py-0.5 rounded border border-transparent hover:border-[var(--color-border)] focus:border-[var(--color-accent)] outline-none"
      />

      <span className="text-[10px] text-[var(--color-text-dim)] shrink-0">
        {count} {count === 1 ? "session" : "sessions"}
      </span>

      {confirming ? (
        <span className="flex items-center gap-1 shrink-0">
          <button
            onClick={onDelete}
            className="text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-error)] text-[var(--color-error)]"
          >
            delete?
          </button>
          <button
            onClick={() => setConfirming(false)}
            className="text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)]"
          >
            no
          </button>
        </span>
      ) : (
        <button
          onClick={() => setConfirming(true)}
          className="shrink-0 text-[var(--color-text-dim)] hover:text-[var(--color-error)]"
          title="Delete tag"
        >
          ×
        </button>
      )}
    </div>
  );
}
