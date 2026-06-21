import { useEffect, useState } from "react";
import TerminalView from "./TerminalView";

/** Keeps one mounted xterm per session opened, so each session's live terminal
 *  and scrollback persist across tab/session switches. Shows the active one. */
export default function TerminalPane({
  sessionId,
  filePath,
  cwd,
  tabActive,
}: {
  sessionId: string | null;
  filePath: string | null;
  cwd?: string;
  tabActive: boolean;
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
      <div className="flex items-center justify-center h-full text-[var(--color-text-dim)]">
        Open or create a session to start its terminal.
      </div>
    );
  }

  return (
    <div className="h-full w-full bg-[#0a0a0a]">
      {Object.entries(opened).map(([sid, v]) => (
        <TerminalView
          key={sid}
          sessionId={sid}
          filePath={v.filePath}
          cwd={v.cwd}
          active={tabActive && sid === sessionId}
        />
      ))}
    </div>
  );
}
