import { Fragment, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X } from "lucide-react";
import type { SessionSummary } from "../types";
import { useSessionMeta } from "../hooks/useSessionMeta";
import { sessionTitle } from "../lib/sessionMeta";
import type { OpenSession } from "../lib/openSessions";
import ConfirmStopDialog from "./ConfirmStopDialog";

/** The Open tab: every session whose `claude` runs in a terminal, in any window, each closable. */
export default function OpenSessionsPane({
  rows,
  onClose,
  onOpen,
}: {
  rows: OpenSession[];
  onClose: (id: string) => void;
  /** `newCwd` is set for a session claude hasn't written yet, which the terminal must start in that dir. */
  onOpen: (s: SessionSummary, newCwd?: string) => void;
}) {
  const { meta } = useSessionMeta();
  const [confirm, setConfirm] = useState<SessionSummary | null>(null);
  const idle = rows.filter((r) => !r.working);
  const close = onClose;
  const grouped = rows.some((r) => r.mine) && rows.some((r) => !r.mine);

  return (
    <div className="flex flex-col flex-1 min-h-0">
      <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--color-border)] text-xs">
        <span className="text-[var(--color-text-dim)]">{rows.length} open</span>
        <button
          disabled={idle.length === 0}
          onClick={() => idle.forEach((r) => close(r.session.id))}
          title="Close every terminal Claude isn't working in"
          className="px-2 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:text-[var(--color-text)] hover:border-[var(--color-accent)] disabled:opacity-40 disabled:pointer-events-none"
        >
          Close idle
        </button>
      </div>
      {rows.length === 0 ? (
        <div className="p-4 text-xs text-[var(--color-text-dim)]">No Claude terminals are open.</div>
      ) : (
        <div className="flex-1 min-h-0 overflow-y-auto p-2 space-y-1">
          {rows.map(({ session: s, working, owner, mine, pending }, i) => (
            <Fragment key={s.id}>
              {grouped && (i === 0 || rows[i - 1].mine !== mine) && (
                <div className={`px-1 ${i === 0 ? "" : "pt-2 "}text-[10px] uppercase tracking-wide text-[var(--color-text-dim)]`}>
                  {mine ? "This window" : "Other windows"}
                </div>
              )}
              <div
                role="button"
                onClick={() => onOpen(s, pending ? s.cwd ?? undefined : undefined)}
                className="relative pl-4 pr-2 py-1.5 rounded border border-[var(--color-border)] bg-[var(--color-bg)] hover:border-[var(--color-accent)] cursor-pointer flex items-start gap-2"
              >
                {working && (
                  <span
                    className="absolute left-1.5 top-3 h-1.5 w-1.5 rounded-full bg-green-500 animate-pulse"
                    title="Claude is working"
                  />
                )}
                <div className="min-w-0 flex-1">
                  {pending && !meta.sessions[s.id]?.alias?.trim() ? (
                    <div className="text-xs italic text-[var(--color-text-dim)] truncate">{s.title}</div>
                  ) : (
                    <div className="text-xs text-[var(--color-text)] truncate">{sessionTitle(meta, s.id, s.title)}</div>
                  )}
                  <div className="text-[10px] text-[var(--color-text-dim)] truncate">
                    {s.project_label} ·{" "}
                    {mine ? (
                      "this window"
                    ) : (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          invoke("focus_window", { label: owner }).catch(() => {});
                        }}
                        className="underline hover:text-[var(--color-text)]"
                        title="Bring that window forward"
                      >
                        other window
                      </button>
                    )}
                  </div>
                </div>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    if (working) setConfirm(s);
                    else close(s.id);
                  }}
                  title="Close its terminal"
                  className="shrink-0 mt-0.5 text-[var(--color-text-dim)] hover:text-[var(--color-error)]"
                >
                  <X size={12} strokeWidth={2} />
                </button>
              </div>
            </Fragment>
          ))}
        </div>
      )}
      {confirm && (
        <ConfirmStopDialog
          kind="terminal"
          sessions={[confirm]}
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            close(confirm.id);
            setConfirm(null);
          }}
        />
      )}
    </div>
  );
}
