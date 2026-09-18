import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { X } from "lucide-react";
import type { SessionSummary } from "../types";
import type { LiveActivity } from "../hooks/useActivity";
import { useSessionMeta } from "../hooks/useSessionMeta";
import { sessionTitle } from "../lib/sessionMeta";
import { openSessions } from "../lib/openSessions";
import ConfirmStopDialog from "./ConfirmStopDialog";
import NewSessionButton from "./NewSessionButton";

/** The Open tab: every session whose `claude` runs in a terminal, in any window, each closable. */
export default function OpenSessionsPane({
  sessions,
  activity,
  now,
  onOpen,
  onNewSession,
}: {
  sessions: SessionSummary[];
  activity: Map<string, LiveActivity>;
  now: number;
  onOpen: (s: SessionSummary) => void;
  onNewSession: (cwd: string) => Promise<string>;
}) {
  const { meta } = useSessionMeta();
  const here = getCurrentWebviewWindow().label;
  // Closed here but still hosted in the last poll (every 3 s): hidden rather than lingering.
  const [closing, setClosing] = useState<ReadonlySet<string>>(new Set());
  const [confirm, setConfirm] = useState<SessionSummary | null>(null);

  useEffect(() => {
    setClosing((prev) => {
      const still = [...prev].filter((id) => activity.get(id)?.hosted);
      return still.length === prev.size ? prev : new Set(still);
    });
  }, [activity]);

  const rows = openSessions(sessions, activity, now).filter((r) => !closing.has(r.session.id));
  const idle = rows.filter((r) => !r.working);
  const close = (id: string) => {
    setClosing((prev) => new Set(prev).add(id));
    invoke("terminal_close", { sessionId: id }).catch(() =>
      setClosing((prev) => {
        const next = new Set(prev);
        next.delete(id);
        return next;
      }),
    );
  };

  return (
    <div className="flex flex-col flex-1 min-h-0">
      <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--color-border)] text-xs">
        <span className="text-[var(--color-text-dim)]">{rows.length} open</span>
        <div className="flex items-center gap-1.5">
          <button
            disabled={idle.length === 0}
            onClick={() => idle.forEach((r) => close(r.session.id))}
            title="Close every terminal Claude isn't working in"
            className="px-2 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:text-[var(--color-text)] hover:border-[var(--color-accent)] disabled:opacity-40 disabled:pointer-events-none"
          >
            Close idle
          </button>
          <NewSessionButton onNewSession={onNewSession} align="right" />
        </div>
      </div>
      {rows.length === 0 ? (
        <div className="p-4 text-xs text-[var(--color-text-dim)]">No Claude terminals are open.</div>
      ) : (
        <div className="flex-1 min-h-0 overflow-y-auto p-2 space-y-1">
          {rows.map(({ session: s, working, owner }) => (
            <div
              key={s.id}
              role="button"
              onClick={() => onOpen(s)}
              className="relative pl-4 pr-2 py-1.5 rounded border border-[var(--color-border)] bg-[var(--color-bg)] hover:border-[var(--color-accent)] cursor-pointer flex items-start gap-2"
            >
              {working && (
                <span
                  className="absolute left-1.5 top-3 h-1.5 w-1.5 rounded-full bg-green-500 animate-pulse"
                  title="Claude is working"
                />
              )}
              <div className="min-w-0 flex-1">
                <div className="text-xs text-[var(--color-text)] truncate">{sessionTitle(meta, s.id, s.title)}</div>
                <div className="text-[10px] text-[var(--color-text-dim)] truncate">
                  {s.project_label} ·{" "}
                  {owner == null || owner === here ? (
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
