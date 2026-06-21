import { useState, useCallback, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSessions, useTranscript } from "./hooks/useSessions";
import Sidebar from "./components/Sidebar";
import Transcript from "./components/Transcript";
import TerminalPane from "./components/TerminalPane";
import LineageBanner from "./components/LineageBanner";
import type { ForkRef } from "./types";

const MIN_WIDTH = 200;
const MAX_WIDTH = 600;
const DEFAULT_WIDTH = 320;

export default function App() {
  const { sessions, loading, refresh } = useSessions();
  const { transcript, loading: txLoading, open, close, updateTitle } = useTranscript();
  const [sidebarWidth, setSidebarWidth] = useState(DEFAULT_WIDTH);
  const [collapsed, setCollapsed] = useState(false);
  const [activeTab, setActiveTab] = useState<"manager" | "terminal">("manager");
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
              onRename={async (s, newTitle) => {
                try {
                  await invoke("rename_session", {
                    filePath: s.file_path,
                    newTitle,
                  });
                  // Patch the open transcript's header too, if it's this session.
                  if (transcript?.id === s.id) updateTitle(newTitle);
                  refresh();
                } catch (e) {
                  console.error("rename failed:", e);
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
      <main className="flex-1 overflow-hidden flex flex-col">
        <div className="flex items-center gap-1 px-2 py-1 border-b border-[var(--color-border)] bg-[var(--color-surface)] shrink-0">
          {(["manager", "terminal"] as const).map((t) => (
            <button
              key={t}
              onClick={() => setActiveTab(t)}
              className={
                "px-3 py-1 text-sm rounded " +
                (activeTab === t
                  ? "bg-[var(--color-accent)] text-white"
                  : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]")
              }
            >
              {t === "manager" ? "Manager" : "Terminal"}
            </button>
          ))}
        </div>
        <div className="flex-1 overflow-hidden">
          <div className="h-full" style={{ display: activeTab === "manager" ? "block" : "none" }}>
            <div className="flex flex-col h-full">
        {txLoading ? (
          <div className="flex items-center justify-center h-full text-[var(--color-text-dim)]">
            Loading session…
          </div>
        ) : transcript ? (
          (() => {
            const openSummary = sessions.find((s) => s.id === transcript.id) ?? null;
            const copiedFrom = openSummary?.copied_from
              ? sessions.find((s) => s.id === openSummary.copied_from) ?? null
              : null;
            const copies = sessions.filter((s) => s.copied_from === transcript.id);
            const branchPoints = new Map<string, ForkRef[]>();
            const addBranch = (msg: string, ref: ForkRef) => {
              const arr = branchPoints.get(msg);
              if (arr) arr.push(ref);
              else branchPoints.set(msg, [ref]);
            };
            for (const s of sessions) {
              if (s.forked_from === transcript.id && s.forked_from_message) {
                addBranch(s.forked_from_message, { session: s, kind: "forked" });
              }
              if (s.copied_from === transcript.id && s.copied_from_message) {
                addBranch(s.copied_from_message, { session: s, kind: "copied" });
              }
            }
            return (
              <>
                <LineageBanner
                  copiedFrom={copiedFrom}
                  copies={copies}
                  overlap={openSummary?.copied_overlap ?? null}
                  onOpen={(s) => open(s.file_path, s.project_label)}
                />
                <div className="flex-1 overflow-hidden">
                  <Transcript
                    session={transcript}
                    onBack={close}
                    branchPoints={branchPoints}
                    onOpenSession={(s) => open(s.file_path, s.project_label)}
                  />
                </div>
              </>
            );
          })()
        ) : (
          <div className="flex items-center justify-center h-full text-[var(--color-text-dim)]">
            Select a session to view
          </div>
        )}
            </div>
          </div>
          <div className="h-full" style={{ display: activeTab === "terminal" ? "block" : "none" }}>
            <TerminalPane
              sessionId={transcript?.id ?? null}
              filePath={transcript?.source_path ?? null}
              tabActive={activeTab === "terminal"}
            />
          </div>
        </div>
      </main>
    </div>
  );
}
