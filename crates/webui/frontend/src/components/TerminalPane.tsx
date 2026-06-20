import { useEffect, useState } from "react";
import TerminalView from "./TerminalView";

/** Keeps one mounted xterm per session opened, so each session's live terminal
 *  and scrollback persist across tab/session switches. Shows the active one. */
export default function TerminalPane({
  sessionId,
  filePath,
}: {
  sessionId: string | null;
  filePath: string | null;
}) {
  // sessionId -> filePath for every session a terminal has been opened for.
  const [opened, setOpened] = useState<Record<string, string>>({});

  useEffect(() => {
    if (sessionId && filePath) {
      setOpened((m) => (m[sessionId] ? m : { ...m, [sessionId]: filePath }));
    }
  }, [sessionId, filePath]);

  if (!sessionId) {
    return (
      <div className="flex items-center justify-center h-full text-[var(--color-text-dim)]">
        Open a session in the Manager tab to start its terminal.
      </div>
    );
  }

  return (
    <div className="h-full w-full bg-[#0a0a0a]">
      {Object.entries(opened).map(([sid, fp]) => (
        <TerminalView key={sid} sessionId={sid} filePath={fp} active={sid === sessionId} />
      ))}
    </div>
  );
}
