import { useEffect, useState } from "react";
import TerminalView from "./TerminalView";
import NewSessionButton from "./NewSessionButton";

/** Keeps one mounted xterm per session opened, so each session's live terminal
 *  and scrollback persist across tab/session switches. Shows the active one. */
export default function TerminalPane({
  sessionId,
  filePath,
  cwd,
  tabActive,
  onStatus,
  onNewSession,
}: {
  sessionId: string | null;
  filePath: string | null;
  cwd?: string;
  tabActive: boolean;
  onStatus?: (sessionId: string, s: "live" | "exited" | "busy") => void;
  onNewSession: (cwd: string) => Promise<string>;
}) {
  // sessionId -> { filePath, cwd? } for every session a terminal was opened for.
  const [opened, setOpened] = useState<Record<string, { filePath: string; cwd?: string }>>({});

  useEffect(() => {
    if (sessionId && filePath) {
      setOpened((m) => (m[sessionId] ? m : { ...m, [sessionId]: { filePath, cwd } }));
    }
  }, [sessionId, filePath, cwd]);

  if (!sessionId) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3 text-[var(--color-text-dim)]">
        <span>Open a session, or start a new one here.</span>
        <NewSessionButton onNewSession={onNewSession} label="+ New session" />
      </div>
    );
  }

  // The wrapper's background must match the xterm theme's: xterm fills whole
  // cells, so the partial row left over at the bottom shows this element
  // through. Reading the same variable the terminal theme reads keeps them
  // identical in every theme — the old hardcoded #0a0a0a is what showed as a
  // black bar once the terminal moved to the palette's #0d1117.
  return (
    <div className="h-full w-full bg-[var(--color-bg)]">
      {Object.entries(opened).map(([sid, v]) => (
        <TerminalView
          key={sid}
          sessionId={sid}
          filePath={v.filePath}
          cwd={v.cwd}
          active={tabActive && sid === sessionId}
          onStatus={onStatus ? (s) => onStatus(sid, s) : undefined}
        />
      ))}
    </div>
  );
}
