import { useState, useCallback, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSessions, useTranscript } from "./hooks/useSessions";
import Sidebar from "./components/Sidebar";
import Transcript from "./components/Transcript";

const MIN_WIDTH = 200;
const MAX_WIDTH = 600;
const DEFAULT_WIDTH = 320;

export default function App() {
  const { sessions, loading, refresh } = useSessions();
  const { transcript, loading: txLoading, open, close } = useTranscript();
  const [sidebarWidth, setSidebarWidth] = useState(DEFAULT_WIDTH);
  const [collapsed, setCollapsed] = useState(false);
  const dragging = useRef(false);
  const startX = useRef(0);
  const startW = useRef(0);

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      dragging.current = true;
      startX.current = e.clientX;
      startW.current = sidebarWidth;
    },
    [sidebarWidth]
  );

  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      const delta = e.clientX - startX.current;
      const next = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, startW.current + delta));
      setSidebarWidth(next);
    };
    const onMouseUp = () => {
      dragging.current = false;
    };
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, []);

  return (
    <div className="flex h-screen overflow-hidden">
      {collapsed ? (
        <div className="flex flex-col border-r border-[var(--color-border)] bg-[var(--color-surface)]">
          <button
            onClick={() => setCollapsed(false)}
            className="px-2 py-3 text-[var(--color-text-dim)] hover:text-[var(--color-text)] text-sm"
            title="Expand sidebar"
          >
            ▸
          </button>
        </div>
      ) : (
        <>
          <div style={{ width: sidebarWidth, minWidth: sidebarWidth }} className="flex flex-col">
            <Sidebar
              sessions={sessions}
              loading={loading}
              activeId={transcript?.id ?? null}
              onSelect={(s) => open(s.file_path, s.project_label)}
              onRefresh={refresh}
              onCollapse={() => setCollapsed(true)}
              onDelete={async (s) => {
                try {
                  await invoke("delete_session", { filePath: s.file_path });
                  if (transcript?.id === s.id) close();
                  refresh();
                } catch (e) {
                  console.error("delete failed:", e);
                }
              }}
            />
          </div>
          {/* Drag handle */}
          <div
            onMouseDown={onMouseDown}
            className="w-1 cursor-col-resize hover:bg-[var(--color-accent)] active:bg-[var(--color-accent)] transition-colors shrink-0"
          />
        </>
      )}
      <main className="flex-1 overflow-hidden">
        {txLoading ? (
          <div className="flex items-center justify-center h-full text-[var(--color-text-dim)]">
            Loading session…
          </div>
        ) : transcript ? (
          <Transcript session={transcript} onBack={close} />
        ) : (
          <div className="flex items-center justify-center h-full text-[var(--color-text-dim)]">
            Select a session to view
          </div>
        )}
      </main>
    </div>
  );
}
