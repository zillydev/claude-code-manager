import { useState, useCallback, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSessions, useTranscript } from "./hooks/useSessions";
import Sidebar from "./components/Sidebar";
import Transcript from "./components/Transcript";
import TerminalPane from "./components/TerminalPane";
import TabToggle from "./components/TabToggle";
import LineageBanner from "./components/LineageBanner";
import LineageTree from "./components/LineageTree";
import BookmarkBrowser from "./components/BookmarkBrowser";
import GlobalFind from "./components/GlobalFind";
import Settings from "./components/Settings";
import Spinner from "./components/Spinner";
import { usePersistentState } from "./hooks/usePersistentState";
import { useActivity } from "./hooks/useActivity";
import { useConfig } from "./hooks/useConfig";
import type { ForkRef } from "./types";

const MIN_WIDTH = 200;
const MAX_WIDTH = 600;

const basename = (p: string) => p.replace(/\/+$/, "").split("/").pop() || p;

export default function App() {
  const { sessions, loading, refresh } = useSessions();
  const { transcript, loading: txLoading, open, close } = useTranscript();
  // Sidebar width/collapsed + the launch default tab are config-backed. Width is
  // mirrored locally for smooth dragging (synced from config except mid-drag).
  const { config, setConfig, loaded: configLoaded } = useConfig();
  const collapsed = config.ui.sidebarCollapsed;
  const [sidebarWidth, setSidebarWidth] = useState(config.ui.sidebarWidth);
  const [activeTab, setActiveTab] = useState<"manager" | "terminal">("manager");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [bookmarksOpen, setBookmarksOpen] = useState(false);
  const [findOpen, setFindOpen] = useState(false);
  // Per-session terminal liveness for THIS window: drives the Terminal tab dot
  // and enables the close-✕. Fed by TerminalView via TerminalPane's onStatus.
  const [terminalStatus, setTerminalStatus] = useState<
    Record<string, "live" | "exited" | "busy">
  >({});
  // The single open session, shared by BOTH tabs: Manager renders its transcript,
  // Terminal runs its `claude` process. `cwd` set ⇒ a freshly-created session whose
  // file doesn't exist until the first turn — the effect below polls for it.
  const [target, setTarget] = useState<{
    sessionId: string;
    filePath: string;
    projectLabel: string;
    cwd?: string;
    /** Message uuid to scroll the transcript to on open (e.g. a fork point). */
    scrollToMessage?: string;
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
  // `scrollToMessage` optionally scrolls the transcript to a specific message
  // (e.g. a fork point opened from the lineage tree).
  const openSession = useCallback(
    (s: { id: string; file_path: string; project_label: string }, scrollToMessage?: string) => {
      setTarget({
        sessionId: s.id,
        filePath: s.file_path,
        projectLabel: s.project_label,
        scrollToMessage,
      });
    },
    [],
  );

  const onTerminalStatus = useCallback(
    (sid: string, status: "live" | "exited" | "busy") =>
      setTerminalStatus((m) => (m[sid] === status ? m : { ...m, [sid]: status })),
    [],
  );

  // Close = kill the PTY; the resulting terminal-exit drives status back to
  // "exited" (clearing the dot) and the pane into its [process exited]+Restart state.
  const closeTerminal = useCallback(() => {
    if (target) invoke("terminal_close", { sessionId: target.sessionId }).catch(() => {});
  }, [target]);

  // Create a new session in `cwd`: a fresh `claude --session-id` in the terminal.
  // Returns the new session's id so the caller can attach metadata to it (the
  // sidebar tags it with any active tag filter).
  const newSession = useCallback(async (cwd: string): Promise<string> => {
    const res = await invoke<{ session_id: string; file_path: string }>("new_session", { cwd });
    setTarget({
      sessionId: res.session_id,
      filePath: res.file_path,
      projectLabel: basename(cwd),
      cwd,
    });
    setActiveTab("terminal");
    return res.session_id;
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
  const widthRef = useRef(sidebarWidth);

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
      widthRef.current = next;
      setSidebarWidth(next);
    };
    const onMouseUp = () => {
      if (!dragging.current) return;
      dragging.current = false;
      // Persist the final width once (not on every mousemove).
      setConfig({ ui: { sidebarWidth: widthRef.current } });
    };
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [setConfig]);

  // Mirror config width locally (initial load + cross-window), except mid-drag.
  useEffect(() => {
    if (!dragging.current) {
      setSidebarWidth(config.ui.sidebarWidth);
      widthRef.current = config.ui.sidebarWidth;
    }
  }, [config.ui.sidebarWidth]);

  // Apply the launch-only default tab once the real config has loaded.
  const didInitTab = useRef(false);
  useEffect(() => {
    if (configLoaded && !didInitTab.current) {
      didInitTab.current = true;
      setActiveTab(config.ui.defaultTab);
    }
  }, [configLoaded, config.ui.defaultTab]);

  // Cmd+E toggles Manager ↔ Terminal; Cmd+B toggles the sidebar; Cmd+, opens
  // Settings (the macOS Preferences shortcut). Cmd (not Ctrl) so they never
  // collide with the terminal — Ctrl+E/Ctrl+B are line-editing keys in
  // claude/shells. Capture phase so it beats xterm when the terminal is focused.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // ⌘⇧F — global find. Checked before the no-shift guard below.
      if (e.metaKey && e.shiftKey && !e.ctrlKey && !e.altKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        e.stopPropagation();
        setFindOpen(true);
        return;
      }
      if (!e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
      const k = e.key.toLowerCase();
      if (k === "e") {
        e.preventDefault();
        e.stopPropagation();
        setActiveTab((t) => (t === "manager" ? "terminal" : "manager"));
      } else if (k === "b") {
        e.preventDefault();
        e.stopPropagation();
        setConfig({ ui: { sidebarCollapsed: !collapsed } });
      } else if (k === ",") {
        e.preventDefault();
        e.stopPropagation();
        setSettingsOpen(true);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [collapsed, setConfig]);

  const terminalLive = !!target && terminalStatus[target.sessionId] === "live";

  return (
    <div className="flex h-screen overflow-hidden">
      {collapsed ? (
        <div className="flex flex-col border-r border-[var(--color-border)] bg-[var(--color-surface)]">
          <button
            onClick={() => setConfig({ ui: { sidebarCollapsed: false } })}
            className="px-3 py-3 text-lg leading-none text-[var(--color-text-dim)] hover:text-[var(--color-accent)] hover:bg-[var(--color-surface-hover)]"
            title="Expand sidebar (⌘B)"
          >
            »
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
              onSelect={(s, messageUuid) => openSession(s, messageUuid)}
              onCollapse={() => setConfig({ ui: { sidebarCollapsed: true } })}
              onDelete={async (s) => {
                try {
                  await invoke("delete_session", { filePath: s.file_path });
                  if (target?.sessionId === s.id) setTarget(null);
                  refresh();
                } catch (e) {
                  console.error("delete failed:", e);
                }
              }}
              onNewSession={newSession}
              onViewTree={(s) => setLineageFor(s.id)}
              onOpenSettings={() => setSettingsOpen(true)}
              onOpenBookmarks={() => setBookmarksOpen(true)}
              onOpenFind={() => setFindOpen(true)}
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
                    branchPoints={branchPoints}
                    onOpenSession={(s) => openSession(s)}
                    onViewTree={hasLineage ? () => setLineageFor(transcript.id) : null}
                    scrollToMessage={target?.scrollToMessage ?? null}
                    visible={activeTab === "manager"}
                    tabs={
                      <TabToggle
                        activeTab={activeTab}
                        onSelect={setActiveTab}
                        terminalLive={terminalLive}
                        onClose={closeTerminal}
                      />
                    }
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
            <div className="flex flex-col h-full">
              {/* Slim header mirrors the transcript's: session title + the toggle. */}
              <div className="flex items-center gap-3 px-4 py-2.5 border-b border-[var(--color-border)] bg-[var(--color-surface)] shrink-0">
                {(transcript?.project_label ?? target?.projectLabel) && (
                  <span className="font-medium text-[var(--color-accent)]">
                    {transcript?.project_label ?? target?.projectLabel}
                  </span>
                )}
                {transcript?.title && (
                  <span className="text-[var(--color-text)] font-medium truncate">
                    {transcript.title}
                  </span>
                )}
                <div className="ml-auto">
                  <TabToggle
                    activeTab={activeTab}
                    onSelect={setActiveTab}
                    terminalLive={terminalLive}
                    onClose={closeTerminal}
                  />
                </div>
              </div>
              <div className="flex-1 overflow-hidden">
                <TerminalPane
                  sessionId={target?.sessionId ?? null}
                  filePath={target?.filePath ?? null}
                  cwd={target?.cwd}
                  tabActive={activeTab === "terminal"}
                  onStatus={onTerminalStatus}
                />
              </div>
            </div>
          </div>
        </div>
      </main>
      {lineageFor && (
        <LineageTree
          sessions={sessions}
          centerId={lineageFor}
          onOpen={(s, scrollToMessage) => {
            openSession(s, scrollToMessage);
            setLineageFor(null);
          }}
          onClose={() => setLineageFor(null)}
        />
      )}
      {findOpen && (
        <GlobalFind
          sessions={sessions}
          onOpen={(s, messageUuid) => {
            openSession(s, messageUuid);
            setFindOpen(false);
          }}
          onClose={() => setFindOpen(false)}
        />
      )}
      {bookmarksOpen && (
        <BookmarkBrowser
          sessions={sessions}
          onOpen={(s, messageUuid) => {
            openSession(s, messageUuid);
            setBookmarksOpen(false);
          }}
          onClose={() => setBookmarksOpen(false)}
        />
      )}
      {settingsOpen && <Settings onClose={() => setSettingsOpen(false)} />}
    </div>
  );
}
