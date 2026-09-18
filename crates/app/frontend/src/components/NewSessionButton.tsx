import { useState } from "react";
import Spinner from "./Spinner";

function shortenPath(p: string): string {
  return p.replace(/^\/Users\/[^/]+/, "~");
}

/** The "+ New" button + popover that starts a fresh `claude` session in a chosen
 *  directory. Shared by the Sessions sidebar, the Open tab, and the terminal
 *  empty state so every "create a session" affordance behaves identically. */
export default function NewSessionButton({
  onNewSession,
  selectedCwd = null,
  onCreated,
  align = "left",
  label = "+ New",
  className = "",
}: {
  onNewSession: (cwd: string) => Promise<string>;
  /** A directory the surrounding context already implies, offered as one click. */
  selectedCwd?: string | null;
  /** Runs with the new session's id once it's created (e.g. to attach tags). */
  onCreated?: (id: string) => void;
  align?: "left" | "right";
  label?: string;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const [path, setPath] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const create = async (cwd: string) => {
    const c = cwd.trim();
    if (!c) return;
    setCreating(true);
    try {
      const id = await onNewSession(c);
      onCreated?.(id);
      setOpen(false);
      setPath("");
      setErr(null);
    } catch (e) {
      setErr(String(e));
    } finally {
      setCreating(false);
    }
  };

  return (
    <div className={`relative ${className}`}>
      <button
        onClick={() => setOpen((o) => !o)}
        className="text-xs px-2 py-1 rounded bg-[var(--color-accent)] text-white"
        title="New session"
      >
        {label}
      </button>
      {open && (
        <div
          className={`absolute top-full mt-1 z-30 w-64 p-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] shadow-lg space-y-2 ${
            align === "right" ? "right-0" : "left-0"
          }`}
        >
          {selectedCwd && (
            <button
              onClick={() => create(selectedCwd)}
              disabled={creating}
              className="w-full text-left text-xs px-2 py-1 rounded bg-[var(--color-bg)] border border-[var(--color-border)] hover:border-[var(--color-accent)] truncate disabled:opacity-50"
              title={selectedCwd}
            >
              New in {shortenPath(selectedCwd)}
            </button>
          )}
          <div className="flex gap-1">
            <input
              type="text"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") create(path);
              }}
              placeholder="paste a directory path…"
              className="flex-1 bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-2 py-1 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
            />
            <button
              onClick={() => create(path)}
              disabled={!path.trim() || creating}
              className="text-xs px-2 py-1 rounded bg-[var(--color-accent)] text-white disabled:opacity-40 inline-flex items-center gap-1.5"
            >
              {creating && <Spinner />}
              Create
            </button>
          </div>
          {err && <div className="text-[var(--color-error)] text-[10px]">{err}</div>}
        </div>
      )}
    </div>
  );
}
