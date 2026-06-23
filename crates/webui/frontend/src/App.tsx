import { useState, useCallback, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSessions, useTranscript } from "./hooks/useSessions";
import Sidebar from "./components/Sidebar";
import Transcript from "./components/Transcript";
import TerminalPane from "./components/TerminalPane";
import LineageBanner from "./components/LineageBanner";
import LineageTree from "./components/LineageTree";
import Spinner from "./components/Spinner";
import { usePersistentState } from "./hooks/usePersistentState";
import { useActivity } from "./hooks/useActivity";
import type { ForkRef } from "./types";

const MIN_WIDTH = 200;
const MAX_WIDTH = 600;
const DEFAULT_WIDTH = 320;

const basename = (p: string) => p.replace(/\/+$/, "").split("/").pop() || p;

export default function App() {
  const { sessions, loading, refresh } = useSessions();
  const { transcript, loading: txLoading, open, close, updateTitle } = useTranscript();
  const [sidebarWidth, setSidebarWidth] = useState(DEFAULT_WIDTH);
  const [collapsed, setCollapsed] = useState(false);
  const [activeTab, setActiveTab] = useState<"manager" | "terminal">("manager");
  // The single open session, shared by BOTH tabs: Manager renders its transcript,
  // Terminal runs its `claude` process. `cwd` set ⇒ a freshly-created session whose
  // file doesn't exist until the first turn — the effect below polls for it.
  const [target, setTarget] = useState<{
    sessionId: string;
    filePath: string;
    projectLabel: string;
    cwd?: string;
  } | null>(null);

  // Live per-session activity (epoch s of last real turn) for the sidebar
  // running dot, and the per-session last-viewed times that drive "unread".
  const activity = useActivity();
  const [lastViewed, setLastViewed] = usePersistentState<Record<string, number>>(
    "cm:lastViewed",
    {},
  );
  // Session whose lineage tree is shown in the overlay (null = closed).
  const [lineageFor, setLineageFor] = useState<string | null>(null);

  // Open an existing session (Manager + Terminal both follow `target`).
  const openSession = useCallback(
    (s: { id: string; file_path: string; project_label: string }) => {
      setTarget({ sessionId: s.id, filePath: s.file_path, projectLabel: s.project_label });
    },
    [],
  );

  // Create a new session in `cwd`: a fresh `claude --session-id` in the terminal.
  const newSession = useCallback(async (cwd: string) => {
    const res = await invoke<{ session_id: string; file_path: string }>("new_session", { cwd });
    setTarget({
      sessionId: res.session_id,
      filePath: res.file_path,
      projectLabel: basename(cwd),
      cwd,
    });
    setActiveTab("terminal");
  }, []);

  // Load `target` into the Manager. An existing session's file is there immediately;
  // a new session's appears only after claude writes the first turn — so poll for it,
  // clearing any stale transcript meanwhile, and refresh the sidebar once it lands.
  // Keyed on `target`: switching sessions cancels the previous (possibly polling) run.
  useEffect(() => {
    if (!target) {
      close();
      return;
    }
    let cancelled = false;
    let polled = false;
    const load = async () => {
      const exists = await invoke<boolean>("path_exists", { path: target.filePath });
      if (cancelled) return;
      if (exists) {
        open(target.filePath, target.projectLabel);
        const nowSecs = Math.floor(Date.now() / 1000);
        setLastViewed((m) => ({ ...m, [target.sessionId]: nowSecs }));
        if (polled) refresh(); // new session just appeared in its project dir
      } else {
        if (!polled) close(); // clear the previously-open session so Manager isn't stale
        polled = true;
        setTimeout(load, 700);
      }
    };
    load();
    return () => {
      cancelled = true;
    };
  }, [target, open, close, refresh, setLastViewed]);

  // While a session is open, keep its last-viewed time abreast of incoming turns
  // so live-tailed content in the session you're watching isn't flagged unread.
  useEffect(() => {
    if (!target) return;
    const live = activity.get(target.sessionId)?.at;
    if (live == null) return;
    setLastViewed((m) =>
      live > (m[target.sessionId] ?? 0) ? { ...m, [target.sessionId]: live } : m,
    );
  }, [activity, target, setLastViewed]);

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

  // Cmd+E toggles Manager ↔ Terminal. Cmd (not Ctrl) so it never collides with
  // the terminal — Ctrl+E is "move to end of line" in claude/shells. Capture
  // phase so it beats xterm when the terminal is focused.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "e") {
        e.preventDefault();
        e.stopPropagation();
        setActiveTab((t) => (t === "manager" ? "terminal" : "manager"));
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
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
              activeId={target?.sessionId ?? null}
              activity={activity}
              lastViewed={lastViewed}
              onSelect={(s) => openSession(s)}
              onRefresh={refresh}
              onCollapse={() => setCollapsed(true)}
              onDelete={async (s) => {
                try {
                  await invoke("delete_session", { filePath: s.file_path });
                  if (target?.sessionId === s.id) setTarget(null);
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
              onNewSession={newSession}
              onViewTree={(s) => setLineageFor(s.id)}
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
          <div className="flex flex-col items-center justify-center h-full gap-2 text-[var(--color-text-dim)]">
            <Spinner size="md" />
            <span>Loading session…</span>
          </div>
        ) : transcript && transcript.id === target?.sessionId ? (
          (() => {
            const openSummary = sessions.find((s) => s.id === transcript.id) ?? null;
            const copiedFrom = openSummary?.copied_from
              ? sessions.find((s) => s.id === openSummary.copied_from) ?? null
              : null;
            const copies = sessions.filter((s) => s.copied_from === transcript.id);
            const hasLineage =
              !!(openSummary && (openSummary.forked_from || openSummary.copied_from)) ||
              sessions.some(
                (s) => s.forked_from === transcript.id || s.copied_from === transcript.id,
              );
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
                  onOpen={(s) => openSession(s)}
                />
                <div className="flex-1 overflow-hidden">
                  <Transcript
                    session={transcript}
                    onBack={close}
                    branchPoints={branchPoints}
                    onOpenSession={(s) => openSession(s)}
                    onViewTree={hasLineage ? () => setLineageFor(transcript.id) : null}
                    visible={activeTab === "manager"}
                  />
                </div>
              </>
            );
          })()
        ) : target ? (
          <div className="flex flex-col items-center justify-center h-full gap-2 text-[var(--color-text-dim)]">
            <div>New session in {target.cwd ? basename(target.cwd) : target.projectLabel}.</div>
            <button
              onClick={() => setActiveTab("terminal")}
              className="px-3 py-1 rounded bg-[var(--color-accent)] text-white text-sm"
            >
              Open Terminal to begin
            </button>
            <div className="text-xs">It appears here once you send the first message.</div>
          </div>
        ) : (
          <div className="flex items-center justify-center h-full text-[var(--color-text-dim)]">
            Select a session to view
          </div>
        )}
            </div>
          </div>
          <div className="h-full" style={{ display: activeTab === "terminal" ? "block" : "none" }}>
            <TerminalPane
              sessionId={target?.sessionId ?? null}
              filePath={target?.filePath ?? null}
              cwd={target?.cwd}
              tabActive={activeTab === "terminal"}
            />
          </div>
        </div>
      </main>
      {lineageFor && (
        <LineageTree
          sessions={sessions}
          centerId={lineageFor}
          onOpen={(s) => {
            openSession(s);
            setLineageFor(null);
          }}
          onClose={() => setLineageFor(null)}
        />
      )}
    </div>
  );
}
