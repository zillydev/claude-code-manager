import { useEffect, useState } from "react";
import TerminalView from "./TerminalView";
import { keptTerminals, type TerminalStatus } from "../lib/terminalViews";

/** Keeps a mounted xterm for the current session and for every session whose `claude` still runs, so live
 *  terminals and their scrollback persist across tab/session switches. Shows the active one. */
export default function TerminalPane({
  sessionId,
  filePath,
  cwd,
  tabActive,
  onStatus,
  onTakeOver,
}: {
  sessionId: string | null;
  filePath: string | null;
  cwd?: string;
  tabActive: boolean;
  onStatus?: (sessionId: string, s: TerminalStatus) => void;
  onTakeOver?: (sessionId: string) => void;
}) {
  // sessionId -> { filePath, cwd? } for every session a terminal was opened for.
  const [opened, setOpened] = useState<Record<string, { filePath: string; cwd?: string }>>({});
  const [status, setStatus] = useState<Record<string, TerminalStatus>>({});

  useEffect(() => {
    if (sessionId && filePath) {
      setOpened((m) => (m[sessionId] ? m : { ...m, [sessionId]: { filePath, cwd } }));
    }
  }, [sessionId, filePath, cwd]);

  // Leaving a session unmounts its terminal unless claude still runs there; an xterm per browsed session added up.
  useEffect(() => {
    setOpened((m) => keptTerminals(m, sessionId, status));
  }, [sessionId, status]);

  const reportStatus = (sid: string, s: TerminalStatus) => {
    setStatus((m) => (m[sid] === s ? m : { ...m, [sid]: s }));
    onStatus?.(sid, s);
  };

  if (!sessionId) {
    return (
      <div className="flex items-center justify-center h-full text-[var(--color-text-dim)]">
        Open or create a session to start its terminal.
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
          onStatus={(s) => reportStatus(sid, s)}
          onTakeOver={() => onTakeOver?.(sid)}
        />
      ))}
    </div>
  );
}
