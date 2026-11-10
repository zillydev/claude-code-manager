import { useState, useRef, useEffect, useCallback, useMemo, memo, Fragment, type ReactNode } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkBreaks from "remark-breaks";
import rehypeHighlight from "rehype-highlight";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import type {
  RenderedSession,
  RenderedTurn,
  RenderedBlock,
  SessionStats,
  ForkRef,
  SessionSummary,
} from "../types";
import ToolCall from "./ToolCall";
import { ImageStrip } from "./InlineImage";
import { ImagePathContext, MD_IMAGE_COMPONENTS, mdUrlTransform, useImagePathCtx } from "./PathImage";
import { AnsiText } from "./AnsiText";
import CustomScrollbar from "./CustomScrollbar";
import ContextMenu, { useContextMenu } from "./ContextMenu";
import FindBar from "./FindBar";
import { useFind } from "../hooks/useFind";
import { useEditor } from "../hooks/useEditor";
import CommitResultBanner from "./CommitResultBanner";
import ForkMarker from "./ForkMarker";
import Dropdown, { menuItemClass } from "./Dropdown";
import { splitInsights } from "../lib/insights";
import Spinner from "./Spinner";
import { ArrowDown, ArrowUp, Ellipsis, GitFork } from "lucide-react";
import { useWindowView } from "../hooks/useWindowView";
import { useSessionMeta } from "../hooks/useSessionMeta";
import { entryTags } from "../lib/sessionMeta";
import { makeSegId } from "../lib/segments";
import { isNearBottom } from "../lib/scroll";
import { loadPosition, pickAnchor, savePosition, type ScrollAnchor } from "../lib/scrollMemory";
import { applyMode, DEFAULT_MODE, lineCount, MODES, SCROLL_PX, type AppearanceMode, type Pad } from "../lib/appearance";
import BodyBox, { PY_2 } from "./BodyBox";
import { ansiText } from "../lib/ansi";
import { locateJump, turnKeys, turnUuids, type JumpTarget } from "../lib/jumpTarget";
import { seedQuery } from "../lib/findHandoff";
import { SessionHeading } from "./SessionHeading";
import { effortSummary, shortModel, turnModelLabel } from "../lib/modelLabel";
import { captureTurn } from "../lib/messageImage";

/** Progress of "Copy as image" on one turn, shown in its action row. */
type ImageCopy = { uuid: string; state: "busy" | "done" | "error"; error?: string };

interface Props {
  session: RenderedSession;
  branchPoints?: Map<string, ForkRef[]>;
  onOpenSession?: (s: SessionSummary) => void;
  /** Open the lineage tree for this session; omitted/null when it has no lineage. */
  onViewTree?: (() => void) | null;
  /** Scroll+flash request (uuid + a nonce that bumps per request, so the same
   *  message can be re-requested); null = jump to bottom on open. */
  scrollToMessage?: { uuid: string; n: number; term?: string } | null;
  /** Whether the transcript's tab is currently showing (false when on Terminal). */
  visible?: boolean;
  /** The Manager/Terminal toggle, slotted into the header's right cluster. */
  tabs?: ReactNode;
  /** ↗ in the find bar: open the global find pinned to this session with `query`. */
  onOpenFind?: (sessionId: string, query: string) => void;
  /** Open a session by id once it is in the session list — used to land on a fork just created. */
  onOpenSessionId?: (id: string) => void;
  /** True while showing a branch the session rewound away (a frozen view). */
  viewingBranch?: boolean;
  /** Re-render the session along the branch this message sits on. */
  onShowBranch?: (uuid: string) => void;
  /** Back to the branch the file ends with. */
  onExitBranch?: () => void;
}

export default function Transcript({
  session,
  branchPoints,
  onOpenSession,
  onViewTree,
  scrollToMessage,
  visible = true,
  tabs,
  onOpenFind,
  onOpenSessionId,
  viewingBranch = false,
  onShowBranch,
  onExitBranch,
}: Props) {
  const { mode, modeSeq, detailsOpen: statsOpen, setMode, setDetailsOpen } = useWindowView();
  // Local alias/note for this session (claude-code-manager metadata, not the .jsonl).
  const { meta, setSessionTags, toggleBookmark } = useSessionMeta();
  const metaEntry = meta.sessions[session.id];
  const bottomRef = useRef<HTMLDivElement>(null);
  const turnsRef = useRef<HTMLDivElement>(null);
  // Tail-follow state: whether the user is currently pinned to the bottom.
  // Read from scroll events (the user's last position) rather than measured
  // after a turn is appended — appended content grows scrollHeight without
  // firing a scroll, so a post-append measurement would misreport "at bottom".
  const atBottomRef = useRef(true);
  // The same fact in a form that renders. The ref is what effects and the scroll
  // listener read synchronously (that listener is attached once and must not
  // re-subscribe on every change); the state is what shows/hides the jump
  // button. `setPinned` is the ONLY writer of either, so they cannot drift.
  const [atBottom, setAtBottom] = useState(true);
  // Mirror flag for the ↑ button (true while the viewport is at the top).
  const [atTop, setAtTop] = useState(true);
  const setPinned = useCallback((v: boolean) => {
    atBottomRef.current = v;
    setAtBottom(v); // same value ⇒ React bails, so scrolling costs no re-render
  }, []);
  // Re-pins as well as scrolls: the point of the button is to resume following
  // the live tail, not just to move the viewport once.
  const jumpToLatest = useCallback(() => {
    setPinned(true);
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [setPinned]);
  // Unpins via the scroll listener (top ≠ following the tail).
  const jumpToTop = useCallback(() => {
    turnsRef.current?.scrollTo({ top: 0, behavior: "smooth" });
  }, []);
  const prevSessionId = useRef<string | null>(null);
  // A remembered position waiting to be put back once the view can be measured, which it can't while hidden.
  const restoreRef = useRef<ScrollAnchor | null>(null);
  const restoreRaf = useRef(0);
  // The latest position measured while scrolling, written once scrolling settles, on switching away, or on unmount.
  const pendingSave = useRef<{ id: string; anchor: ScrollAnchor | null } | null>(null);
  const saveTimer = useRef(0);
  const scrollKeyRef = useRef({ id: session.id, branch: viewingBranch, visible });
  scrollKeyRef.current = { id: session.id, branch: viewingBranch, visible };
  const flushSave = useCallback(() => {
    window.clearTimeout(saveTimer.current);
    const p = pendingSave.current;
    pendingSave.current = null;
    if (p) savePosition(p.id, p.anchor);
  }, []);
  // Put a remembered position back after layout; a message no longer drawn falls back to the latest.
  const scheduleRestore = useCallback(() => {
    cancelAnimationFrame(restoreRaf.current);
    restoreRaf.current = requestAnimationFrame(() => {
      restoreRaf.current = requestAnimationFrame(() => {
        restoreRaf.current = 0;
        const anchor = restoreRef.current;
        restoreRef.current = null;
        const scroller = turnsRef.current;
        if (!anchor || !scroller) return;
        const el = scroller.querySelector<HTMLElement>(`[data-msg-uuids~="${anchor.uuid}"]`);
        if (!el) {
          setPinned(true);
          bottomRef.current?.scrollIntoView();
          return;
        }
        scroller.scrollTop += el.getBoundingClientRect().top - scroller.getBoundingClientRect().top + anchor.offset;
      });
    });
  }, [setPinned]);
  const editor = useEditor();
  const sourcePath = session.source_path;

  // This session's bookmarks, in transcript order, labelled from the rendered
  // turns — no backend needed while the session is open.
  const bookmarkUuids = metaEntry?.bookmarks;
  const bookmarkSet = useMemo(() => new Set(bookmarkUuids ?? []), [bookmarkUuids]);

  // The turns actually rendered. Declared before the scroll effects because
  // they key on its length — switching mode reshapes this (not session.turns).
  // MUST be memoised: useFind's recompute effect keys on this list and sets
  // state — a per-render identity would loop render → effect → setState.
  const visibleTurns = useMemo(
    () => applyMode(session.turns, mode),
    [mode, session.turns],
  );
  // A mode switch keeps the turn at the top of the view in place. Measured during the render that
  // switches, while the DOM still shows the old mode; put back once the new one has laid out.
  const shownMode = useRef(mode);
  const modeAnchor = useRef<ScrollAnchor | null>(null);
  if (shownMode.current !== mode) {
    shownMode.current = mode;
    modeAnchor.current = !atBottomRef.current && turnsRef.current ? viewAnchor(turnsRef.current) : null;
  }
  useEffect(() => {
    const anchor = modeAnchor.current;
    modeAnchor.current = null;
    if (!anchor) return;
    let raf2 = 0;
    const raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => {
        const scroller = turnsRef.current;
        const el = scroller?.querySelector<HTMLElement>(`[data-msg-uuids~="${anchor.uuid}"]`);
        if (!scroller || !el) return;
        scroller.scrollTop += el.getBoundingClientRect().top - scroller.getBoundingClientRect().top + anchor.offset;
      });
    });
    return () => {
      cancelAnimationFrame(raf1);
      cancelAnimationFrame(raf2);
    };
  }, [mode]);
  // Why a jump landed nowhere, when it does. Cleared by the next one.
  const [jumpMiss, setJumpMiss] = useState<JumpTarget | null>(null);
  // Bumped to re-run the jump after leaving a hiding mode, without re-firing it on
  // every live-tail turn (which depending on the turn lists directly would).
  const [rejump, setRejump] = useState(0);
  const turnLists = useRef({ all: session.turns, visible: visibleTurns });
  turnLists.current = { all: session.turns, visible: visibleTurns };
  // In-session find (⌘F). Matches what is rendered in this window's mode.
  const find = useFind(turnsRef, visibleTurns, mode);
  const turnKeyList = useMemo(() => turnKeys(visibleTurns), [visibleTurns]);
  const { open: findOpen, openBar: findOpenBar, close: findClose } = find;
  // Read at jump time; keying the jump on the toggles would re-fire it on every toggle.
  const findOptsRef = useRef(find.opts);
  findOptsRef.current = find.opts;
  // Right-click menu for a turn — the same actions as its ⋯ menu.
  const turnMenu = useContextMenu<RenderedTurn>();
  // Stable handlers so the memoized <Turn>s don't re-render needlessly.
  // useEditor's methods are useCallback([])-stable, and sourcePath is stable.
  // A fork mints a derived session — the same work, continued — so it
  // inherits the source's tags. Tags only: an alias
  // would give two sessions one name, and a note is about THAT session. A failed
  // op returns null, and an untagged source writes nothing.
  // Read the source via a ref so inheritTags — and thus the handlers below,
  // which are props of memoized Turns (see the identity note above) — stay
  // identity-stable: `meta` changes on any tag edit, and the context's
  // setSessionTags is rebuilt on every provider render.
  const tagSrcRef = useRef({ meta, id: session.id, setSessionTags, toggleBookmark });
  tagSrcRef.current = { meta, id: session.id, setSessionTags, toggleBookmark };
  // Stable via the ref for the same reason as inheritTags: context methods are
  // rebuilt every provider render, and this is a prop of memoized Turns.
  const handleToggleBookmark = useCallback((uuid: string) => {
    const { id, toggleBookmark: tb } = tagSrcRef.current;
    tb(id, uuid);
  }, []);
  const [imageCopy, setImageCopy] = useState<ImageCopy | null>(null);
  const imageCopyTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  // One capture at a time: overlapping ones would race for the pasteboard and the shared status slot.
  const imageCopyBusy = useRef(false);
  const handleCopyImage = useCallback(async (uuid: string) => {
    if (imageCopyBusy.current) return;
    const node = turnsRef.current?.querySelector<HTMLElement>(`[data-msg-uuids~="${uuid}"]`);
    if (!node) return;
    imageCopyBusy.current = true;
    clearTimeout(imageCopyTimer.current);
    setImageCopy({ uuid, state: "busy" });
    let next: ImageCopy;
    try {
      await invoke("copy_image_png", await captureTurn(node));
      next = { uuid, state: "done" };
    } catch (e) {
      next = { uuid, state: "error", error: e instanceof Error ? e.message : String(e) };
    }
    imageCopyBusy.current = false;
    setImageCopy(next);
    imageCopyTimer.current = setTimeout(() => setImageCopy(null), 1500);
  }, []);
  useEffect(() => () => clearTimeout(imageCopyTimer.current), []);
  const inheritTags = useCallback((newId: string | undefined) => {
    const { meta: m, id, setSessionTags: set } = tagSrcRef.current;
    const tags = entryTags(m.sessions[id]);
    if (newId && tags.length > 0) set(newId, tags);
  }, []);
  // Through a ref, like inheritTags: handleFork is a prop of memoized Turns and must keep its identity.
  const openIdRef = useRef(onOpenSessionId);
  openIdRef.current = onOpenSessionId;
  const handleFork = useCallback(
    async (forkUuid: string) => {
      const res = await editor.fork(sourcePath, forkUuid);
      inheritTags(res?.new_session_id);
      // A fork keeps the forked message's timestamps, so it sorts among old sessions; opening it scrolls the sidebar there.
      if (res) openIdRef.current?.(res.new_session_id);
    },
    [editor.fork, sourcePath, inheritTags],
  );
  // Track whether the user is pinned to the bottom, so live-tail appends only
  // follow when they already are. Programmatic scrolls fire this too, keeping
  // the flag accurate.
  useEffect(() => {
    const container = turnsRef.current;
    if (!container) return;
    let anchorRaf = 0;
    const onScroll = () => {
      setPinned(isNearBottom(container));
      setAtTop(container.scrollTop < 4); // same-value sets bail in React
      // Measured once a frame however fast scroll events fire. Not measured in a branch view (its position isn't
      // the session's), while a restore is pending, or while hidden, where a sizeless view would read as the bottom.
      if (anchorRaf) return;
      anchorRaf = requestAnimationFrame(() => {
        anchorRaf = 0;
        const { id, branch } = scrollKeyRef.current;
        if (branch || restoreRef.current || container.clientHeight === 0) return;
        pendingSave.current = { id, anchor: isNearBottom(container) ? null : viewAnchor(container) };
        window.clearTimeout(saveTimer.current);
        saveTimer.current = window.setTimeout(flushSave, 250);
      });
    };
    container.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("pagehide", flushSave);
    return () => {
      container.removeEventListener("scroll", onScroll);
      window.removeEventListener("pagehide", flushSave);
      cancelAnimationFrame(anchorRaf);
      cancelAnimationFrame(restoreRaf.current);
      flushSave();
    };
  }, [setPinned, flushSave]);

  // Switching sessions writes the one being left before the new one's scrolling can replace it.
  useEffect(() => flushSave, [session.id, flushSave]);

  // On opening/switching a session, return to where it was left, else jump to the latest — unless we were asked
  // to scroll to a specific message (the effect below handles that). When the
  // rendered set changes within the same session — a live-tail turn, or a
  // mode/find toggle that reshapes height — re-pin to the bottom if we were
  // there (otherwise a toggle leaves scrollTop pointing at now-different content).
  useEffect(() => {
    const isNewSession = prevSessionId.current !== session.id;
    prevSessionId.current = session.id;
    if (isNewSession) {
      restoreRef.current = null;
      const saved = scrollToMessage || scrollKeyRef.current.branch ? null : loadPosition(session.id);
      if (scrollToMessage) {
        setPinned(false); // the scroll-to-message effect takes over
      } else if (saved) {
        setPinned(false);
        restoreRef.current = saved;
        if (scrollKeyRef.current.visible) scheduleRestore();
      } else {
        setPinned(true);
        bottomRef.current?.scrollIntoView();
      }
      // Measure rather than wait for a scroll event: a session that fits the
      // viewport scrolls nowhere, so no event would clear a stale atTop.
      setAtTop((turnsRef.current?.scrollTop ?? 0) < 4);
    } else if (atBottomRef.current) {
      bottomRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [session.id, visibleTurns.length, scrollToMessage?.n, setPinned, scheduleRestore]);

  // Scroll to + flash a specific message (e.g. a fork point opened from the
  // lineage tree). Keyed on the message too, so it also fires when the target is
  // the already-open session. Matches against the turn's full uuid list, so a
  // branch point that is a merged-away assistant event still resolves to its
  // (merged) turn; a couple of rAFs let heavy turns finish laying out first.
  useEffect(() => {
    if (!scrollToMessage) return;
    // An explicit jump outranks a remembered position still waiting to be put back.
    restoreRef.current = null;
    cancelAnimationFrame(restoreRaf.current);
    // Ask the turn data first: a querySelector miss can't tell "this session
    // never shows that message" from "it hasn't laid out yet", and silently
    // doing nothing is exactly what a dead search result looked like.
    const where = locateJump(
      scrollToMessage.uuid,
      turnLists.current.all,
      turnLists.current.visible,
    );
    if (where !== "visible") {
      setJumpMiss(where);
      return;
    }
    setJumpMiss(null);
    let raf2 = 0;
    const raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => {
        // Match the turn that COVERS this message uuid — `~=` matches one token
        // in the space-separated uuid list, so a merged assistant turn is found
        // by any of its constituent event uuids, not just head/fork.
        const el = turnsRef.current?.querySelector<HTMLElement>(
          `[data-msg-uuids~="${scrollToMessage.uuid}"]`,
        );
        if (!el) return;
        setPinned(false);
        // Short messages centre; a message taller than ~80% of the viewport
        // aligns its TOP just under the header instead — centring a huge turn
        // lands the viewport in its middle, which reads as a random spot.
        const scroller = turnsRef.current;
        if (scroller && el.getBoundingClientRect().height > scroller.clientHeight * 0.8) {
          const elTop = el.getBoundingClientRect().top;
          const scrollerTop = scroller.getBoundingClientRect().top;
          scroller.scrollTop += elTop - scrollerTop - 16; // 16px breathing room
        } else {
          el.scrollIntoView({ block: "center" });
        }
        el.classList.add("ring-2", "ring-[var(--color-accent)]", "rounded");
        setTimeout(
          () => el.classList.remove("ring-2", "ring-[var(--color-accent)]", "rounded"),
          1600,
        );
        // A find result carries its search: open the bar on it, starting inside this message.
        const { term, uuid } = scrollToMessage;
        if (term) {
          const turn = turnLists.current.visible.findIndex((t) => turnUuids(t).includes(uuid));
          findOpenBar(seedQuery(term, findOptsRef.current), turn >= 0 ? turn : undefined);
        }
      });
    });
    return () => {
      cancelAnimationFrame(raf1);
      cancelAnimationFrame(raf2);
    };
  }, [session.id, scrollToMessage?.uuid, scrollToMessage?.n, rejump, setPinned, findOpenBar]);

  // A jump that landed nowhere can become possible later: showing a rewound
  // branch re-renders the session under the SAME id, so neither the request nor
  // `session.id` changes and the effect above would never re-run. Gated on a
  // miss being on screen, so the live tail's constant new turn arrays cost
  // nothing — and the effect above clears `jumpMiss`, so this settles at once.
  useEffect(() => {
    if (!scrollToMessage || !jumpMiss) return;
    if (locateJump(scrollToMessage.uuid, session.turns, visibleTurns) === "visible") {
      setRejump((r) => r + 1);
    }
  }, [session.turns, visibleTurns, scrollToMessage, jumpMiss]);

  // A notice about the previous session's jump has nothing to say about this one.
  useEffect(() => setJumpMiss(null), [session.id]);

  // When this tab becomes visible again, put back a position that couldn't be measured while hidden, or restore
  // the bottom if we were pinned there: display:none can drift the scroll position, and live turns may have
  // arrived while the pane was hidden (so the old bottom is no longer the bottom).
  useEffect(() => {
    if (!visible) return;
    if (restoreRef.current) {
      scheduleRestore();
      return;
    }
    if (!atBottomRef.current) return;
    const raf = requestAnimationFrame(() => bottomRef.current?.scrollIntoView());
    return () => cancelAnimationFrame(raf);
  }, [visible, scheduleRestore]);

  // ⌘F opens/refocuses the in-session find bar (Zed's buffer search; ⌘⇧F is
  // the global find, handled in App). Only while this tab is showing — with
  // the Terminal tab up, ⌘F would otherwise open a bar nobody can see. A text
  // selection inside the transcript seeds the query, as in Zed.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey || e.key.toLowerCase() !== "f") return;
      if (!visible) return;
      e.preventDefault();
      const sel = window.getSelection();
      const inTranscript = !!sel && !!sel.anchorNode && !!turnsRef.current?.contains(sel.anchorNode);
      const text = inTranscript ? sel!.toString().trim() : "";
      findOpenBar(text && !text.includes("\n") ? text : null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [visible, findOpenBar]);

  // A different session means different turn indices: drop the bar's state.
  useEffect(() => {
    findClose();
  }, [session.id, findClose]);

  const { stats } = session;
  const imageCtx = useImagePathCtx(session.id, session.turns, stats.cwd);

  return (
    <ImagePathContext.Provider value={imageCtx}>
    <div className="relative flex flex-col h-full">
      {/* Header bar */}
      <div className="flex items-center gap-3 px-4 py-2.5 border-b border-[var(--color-border)] bg-[var(--color-surface)] shrink-0">
        <SessionHeading
          project={session.project_label}
          title={metaEntry?.alias ?? session.title}
          subtitle={metaEntry?.alias ? session.title : undefined}
        />
        <div className="ml-auto flex gap-2 items-center shrink-0">
          {onViewTree && (
            <button
              onClick={onViewTree}
              className="flex items-center gap-1 text-xs px-2 py-1 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors"
              title="View this session's fork/copy lineage"
            >
              <GitFork size={12} strokeWidth={1.75} /> Tree
            </button>
          )}
          <ModeMenu mode={mode} setMode={setMode} />
          <button
            onClick={() => setDetailsOpen(!statsOpen)}
            className={`text-xs px-2 py-1 rounded border ${
              statsOpen
                ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                : "border-[var(--color-border)] text-[var(--color-text-dim)]"
            } hover:border-[var(--color-accent)] transition-colors`}
          >
            {statsOpen ? "Hide details" : "Details"}
          </button>
          {tabs}
        </div>
      </div>

      {/* Session detail panel */}
      {statsOpen && (
        <>
          <StatsPanel
            stats={stats}
            eventCount={session.event_count}
            sessionId={session.id}
          />
          {metaEntry?.note && (
            <div className="border-b border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-2 text-xs text-[var(--color-text-dim)] whitespace-pre-wrap break-words">
              <span className="text-[var(--color-text)] font-medium">Note:</span>{" "}
              {metaEntry.note}
            </div>
          )}
        </>
      )}

      {findOpen && (
        <FindBar
          query={find.query}
          setQuery={find.setQuery}
          opts={find.opts}
          setOpts={find.setOpts}
          count={find.count}
          activeIndex={find.activeIndex}
          error={find.error}
          focusNonce={find.focusNonce}
          onNext={find.next}
          onPrev={find.prev}
          onGoto={find.goTo}
          onClose={find.close}
          onSearchAll={() => onOpenFind?.(session.id, find.query)}
        />
      )}

      {viewingBranch && (
        <div className="mx-4 mb-2 px-3 py-2 rounded border border-[var(--color-accent)] bg-[var(--color-surface)] text-xs flex items-center justify-between gap-3">
          <span className="text-[var(--color-text-dim)]">
            Showing a branch this session rewound away from. It ends here, and new turns go to the current branch.
          </span>
          <button
            onClick={onExitBranch}
            className="shrink-0 px-2 py-1 rounded border border-[var(--color-border)] hover:border-[var(--color-accent)]"
          >
            Back to current
          </button>
        </div>
      )}

      {jumpMiss && (
        <div className="mx-4 mb-2 px-3 py-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] text-xs flex items-center justify-between gap-3">
          <span className="text-[var(--color-text-dim)]">
            {jumpMiss === "mode-hidden"
              ? `That message is hidden by ${mode.name} mode.`
              : "That message is on a branch this session rewound away from."}
          </span>
          <div className="flex items-center gap-2 shrink-0">
            {jumpMiss === "mode-hidden" && (
              <button
                onClick={() => {
                  setMode(DEFAULT_MODE.id);
                  setRejump((r) => r + 1);
                }}
                className="px-2 py-1 rounded border border-[var(--color-border)] hover:border-[var(--color-accent)]"
              >
                Show in Default mode
              </button>
            )}
            {jumpMiss === "off-path" && onShowBranch && scrollToMessage && (
              <button
                onClick={() => onShowBranch(scrollToMessage.uuid)}
                className="px-2 py-1 rounded border border-[var(--color-border)] hover:border-[var(--color-accent)]"
              >
                Show this branch
              </button>
            )}
            <button
              onClick={() => setJumpMiss(null)}
              className="text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
            >
              dismiss
            </button>
          </div>
        </div>
      )}

      {/* Turns (wrapped so the find gutter can overlay the scroller's edge) */}
      <div className="relative flex-1 min-h-0">
      <div ref={turnsRef} className="no-native-scrollbar h-full overflow-y-auto overflow-x-hidden pl-4 pr-5 py-4 space-y-4">
        {editor.result && (
          <CommitResultBanner result={editor.result} onDismiss={editor.dismiss} />
        )}
        {editor.error && (
          <div className="text-[var(--color-error)] text-xs mb-2">{editor.error}</div>
        )}
        {visibleTurns.map((turn, i) => {
          const forkUuid =
            turn.type === "user" || turn.type === "assistant" ? turn.fork_uuid : null;
          const ownUuid =
            turn.type === "user" || turn.type === "assistant" ? turn.uuid : null;
          const forkPending =
            editor.pending?.kind === "fork" && editor.pending.uuid === forkUuid;
          let marks: ForkRef[] | undefined;
          if (branchPoints) {
            if (forkUuid) marks = branchPoints.get(forkUuid);
            if (!marks && ownUuid) marks = branchPoints.get(ownUuid);
          }
          return (
            <Fragment key={turnKeyList[i]}>
              <div onContextMenu={(e) => turnMenu.open(e, turn)}>
                <Turn
                  turn={turn}
                  turnIndex={i}
                  onFork={handleFork}
                  bookmarked={ownUuid !== null && bookmarkSet.has(ownUuid)}
                  onToggleBookmark={handleToggleBookmark}
                  forkPending={!!forkPending}
                  imageCopy={imageCopy && imageCopy.uuid === ownUuid ? imageCopy : null}
                  onCopyImage={handleCopyImage}
                  revealedKeys={find.revealedKeys}
                  mode={mode}
                  modeSeq={modeSeq}
                />
              </div>
              {marks && onOpenSession && (
                <ForkMarker refs={marks} onOpen={onOpenSession} />
              )}
            </Fragment>
          );
        })}
        <div ref={bottomRef} />
      </div>

      <CustomScrollbar scrollerRef={turnsRef} contentKey={visibleTurns.length} />

      {/* Overview ruler: every find match's position, active one brighter.
          pointer-events-none so the scrollbar underneath stays usable. */}
      {findOpen && find.marks.length > 0 && (
        <div className="absolute inset-y-0 right-0 w-[10px] pointer-events-none z-10">
          {find.marks.map((m, i) => (
            <div
              key={i}
              className="absolute rounded-sm"
              style={{
                top: `${m.top * 100}%`,
                right: m.active ? 0 : 1,
                width: m.active ? 9 : 6,
                height: m.active ? 4 : 3,
                backgroundColor: m.active ? "var(--color-find-active)" : "var(--color-system)",
                opacity: m.active ? 1 : 0.55,
              }}
            />
          ))}
        </div>
      )}
      </div>

      {turnMenu.menu && (
        <ContextMenu x={turnMenu.menu.x} y={turnMenu.menu.y} onClose={turnMenu.close}>
          {(() => {
            const turn = turnMenu.menu!.target;
            // Same actions as the per-turn ⋯ menu, on the turns that have
            // a uuid to address (system lines and dividers have none).
            if (turn.type !== "user" && turn.type !== "assistant") {
              return <div className="px-3 py-1.5 text-[11px] text-[var(--color-text-dim)]">No actions</div>;
            }
            const item = (label: string, onClick: () => void, disabled = false) => (
              <button
                role="menuitem"
                className={menuItemClass}
                disabled={disabled}
                onClick={() => {
                  turnMenu.close();
                  onClick();
                }}
              >
                {label}
              </button>
            );
            return (
              <>
                {item("Fork from here", () => handleFork(turn.fork_uuid))}
                {item(bookmarkSet.has(turn.uuid) ? "Remove bookmark" : "Bookmark", () =>
                  handleToggleBookmark(turn.uuid),
                )}
                {item("Copy as image", () => handleCopyImage(turn.uuid), imageCopy?.state === "busy")}
              </>
            );
          })()}
        </ContextMenu>
      )}

      {/* Jump to top / latest. Absolute rather than sticky so they never occupy
          layout in the scroller — a sticky footer would shift the transcript
          every time it appeared. Each hides where it would do nothing (↑ at the
          top, ↓ while pinned to the bottom); a column keeps ↓ anchored in place
          when ↑ appears above it. */}
      {(!atTop || !atBottom) && (
        <div className="absolute bottom-4 right-5 z-10 flex flex-col gap-2">
          {!atTop && (
            <button
              onClick={jumpToTop}
              title="Scroll to top"
              aria-label="Scroll to top"
              className="flex h-8 w-8 items-center justify-center rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-text-dim)] shadow-md transition-colors hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
            >
              <ArrowUp size={16} strokeWidth={1.75} />
            </button>
          )}
          {!atBottom && (
            <button
              onClick={jumpToLatest}
              title="Scroll to latest"
              aria-label="Scroll to latest"
              className="flex h-8 w-8 items-center justify-center rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-text-dim)] shadow-md transition-colors hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
            >
              <ArrowDown size={16} strokeWidth={1.75} />
            </button>
          )}
        </div>
      )}
    </div>
    </ImagePathContext.Provider>
  );
}

function ModeMenu({ mode, setMode }: { mode: AppearanceMode; setMode: (id: string) => void }) {
  return (
    <Dropdown
      align="right"
      title="Appearance mode (⌘⇧D)"
      trigger={mode.name}
      triggerClassName={`text-xs px-2 py-1 rounded border ${
        mode !== DEFAULT_MODE
          ? "border-[var(--color-accent)] text-[var(--color-accent)]"
          : "border-[var(--color-border)] text-[var(--color-text-dim)]"
      } hover:border-[var(--color-accent)] transition-colors whitespace-nowrap`}
    >
      {(close) => (
        <div className="w-64">
          {MODES.map((m) => (
            <button
              key={m.id}
              role="menuitem"
              className={`${menuItemClass} flex items-baseline gap-2`}
              onClick={() => {
                setMode(m.id);
                close();
              }}
            >
              <span className={m === mode ? "" : "opacity-0"}>✓</span>
              <span className="min-w-0">
                <span className="block text-[var(--color-text)]">{m.name}</span>
                <span className="block text-[10px] text-[var(--color-text-dim)]">{m.description}</span>
              </span>
            </button>
          ))}
        </div>
      )}
    </Dropdown>
  );
}

/** The scroller's view as the turn at its top edge and how far into it the view starts. */
function viewAnchor(scroller: HTMLElement): ScrollAnchor | null {
  const turns = scroller.querySelectorAll<HTMLElement>("[data-msg-uuids]");
  return pickAnchor(
    turns.length,
    (i) => {
      const r = turns[i].getBoundingClientRect();
      return { uuid: turns[i].dataset.msgUuids!.split(" ")[0], top: r.top, bottom: r.bottom };
    },
    scroller.getBoundingClientRect().top,
  );
}

function StatsPanel({
  stats,
  eventCount,
  sessionId,
}: {
  stats: SessionStats;
  eventCount: number;
  sessionId: string;
}) {
  const totalTokens =
    stats.tokens.input +
    stats.tokens.output +
    stats.tokens.cache_creation;
  const [copied, setCopied] = useState(false);
  const resumeCmd = `claude --resume ${sessionId}`;
  const copyResume = () => {
    navigator.clipboard.writeText(resumeCmd).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1200);
      },
      () => {}
    );
  };

  return (
    <div className="px-4 py-3 border-b border-[var(--color-border)] bg-[var(--color-surface)] text-xs shrink-0">
      <div className="grid gap-y-1 items-baseline" style={{ gridTemplateColumns: "auto 1fr" }}>
        <span className="text-[var(--color-text-dim)] opacity-60 pr-3 text-right">session</span>
        <span className="flex items-center gap-2 min-w-0">
          <span className="text-[var(--color-text)] font-mono truncate">{sessionId}</span>
          <button
            onClick={copyResume}
            title={`Copy: ${resumeCmd}`}
            className="shrink-0 text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors"
          >
            {copied ? "copied ✓" : "copy resume"}
          </button>
        </span>
        {stats.cwd && (
          <>
            <span className="text-[var(--color-text-dim)] opacity-60 pr-3 text-right">cwd</span>
            <span className="text-[var(--color-text)] font-mono">{shortenPath(stats.cwd)}</span>
          </>
        )}
        {stats.git_branch && (
          <>
            <span className="text-[var(--color-text-dim)] opacity-60 pr-3 text-right">branch</span>
            <span className="text-[var(--color-accent)] font-mono">{stats.git_branch}</span>
          </>
        )}
        {stats.models.length > 0 && (
          <>
            <span className="text-[var(--color-text-dim)] opacity-60 pr-3 text-right">model</span>
            <span className="flex flex-wrap gap-x-3">
              {stats.models.map((m) => {
                const efforts = effortSummary(m);
                return (
                  <span key={m.model}>
                    <span className="text-[var(--color-assistant)] font-mono">{shortModel(m.model)}</span>
                    <span className="text-[var(--color-text-dim)]"> ×{m.turn_count}</span>
                    {efforts && <span className="text-[var(--color-text-dim)]"> ({efforts})</span>}
                  </span>
                );
              })}
            </span>
          </>
        )}
        <>
          <span className="text-[var(--color-text-dim)] opacity-60 pr-3 text-right">context</span>
          <span>
            <span className="text-[var(--color-text)]">{compact(stats.context_tokens)} tokens</span>
            <span className="text-[var(--color-text-dim)] ml-3">current window size</span>
          </span>
        </>
        <>
          <span className="text-[var(--color-text-dim)] opacity-60 pr-3 text-right">lifetime</span>
          <span>
            <span className="text-[var(--color-text)]">{compact(totalTokens)} processed</span>
            <span className="text-[var(--color-text-dim)] ml-3">
              {compact(stats.tokens.input)} in · {compact(stats.tokens.output)} out · {compact(stats.tokens.cache_creation)} cache-w · {compact(stats.tokens.cache_read)} cache-r
            </span>
          </span>
        </>
        <>
          <span className="text-[var(--color-text-dim)] opacity-60 pr-3 text-right">duration</span>
          <span className="text-[var(--color-text)]">
            {stats.duration_secs != null ? formatDuration(stats.duration_secs) : "—"}
            <span className="text-[var(--color-text-dim)] ml-3">{eventCount} events</span>
          </span>
        </>
      </div>
    </div>
  );
}

const Turn = memo(function Turn({
  turn,
  turnIndex,
  onFork,
  bookmarked,
  onToggleBookmark,
  forkPending,
  imageCopy,
  onCopyImage,
  revealedKeys,
  mode,
  modeSeq,
}: {
  turn: RenderedTurn;
  turnIndex: number;
  onFork: (forkUuid: string) => void;
  bookmarked: boolean;
  onToggleBookmark: (uuid: string) => void;
  forkPending: boolean;
  imageCopy: ImageCopy | null;
  onCopyImage: (uuid: string) => void;
  /** Find's reveal-on-navigate set — `"ti:bi"` / `"ti:task"` / `"ti:summary"` keys, see revealKey(). */
  revealedKeys: ReadonlySet<string>;
  mode: AppearanceMode;
  modeSeq: number;
}) {
  const ts = "timestamp" in turn && turn.timestamp ? formatTime(turn.timestamp) : "--:--";
  // Full date shown on hover; undefined ⇒ no tooltip (e.g. compaction turns).
  const tsTitle =
    "timestamp" in turn && turn.timestamp ? formatFullDate(turn.timestamp) : undefined;

  switch (turn.type) {
    // Right-aligned in a bubble, chat-app style. Assistant turns are long and
    // full-width, so a small coloured role label was the only thing separating
    // them — too weak to catch while scrolling fast. Side + surface are both
    // readable from the shape of the page alone, without reading any text.
    case "user":
      return (
        <div className="group flex flex-col items-end" data-msg-uuids={turn.uuids.join(" ")}>
          {/* Mirrored from the assistant header: there the role label starts the
              row at the left edge and the actions trail it, so on this side the
              label ends the row at the right edge and the actions lead. */}
          <div className="flex items-baseline gap-2 mb-1 justify-end">
            <TurnActions
              onFork={() => onFork(turn.fork_uuid)}
              bookmarked={bookmarked}
              onToggleBookmark={() => onToggleBookmark(turn.uuid)}
              messageId={turn.uuid}
              forkPending={forkPending}
              imageCopy={imageCopy}
              onCopyImage={() => onCopyImage(turn.uuid)}
            />
            <span className="text-xs text-[var(--color-text-dim)]" title={tsTitle}>
              {ts}
            </span>
            <span className="font-medium text-[var(--color-user)]">user</span>
            {bookmarked && (
              <span className="text-[var(--color-accent)] text-xs" title="Bookmarked" data-capture-skip>⚑</span>
            )}
          </div>
          <div className="max-w-[75%] min-w-0 flex flex-col items-end">
            {/* wrap-anywhere, not break-words: `overflow-wrap: break-word`
                doesn't reduce an element's intrinsic min-content width, so a
                flex item still refuses to shrink below one unbreakable token —
                a long URL blew the bubble past max-w-75% and out of the pane.
                `anywhere` counts break opportunities in that measurement.
                Measured: 1215px → 525px in a 700px pane. */}
            <div
              data-seg-id={makeSegId(turnIndex, null, "user-text")}
              className="min-w-0 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm whitespace-pre-wrap wrap-anywhere"
            >
              {turn.text}
            </div>
            <ImageStrip images={turn.images} label="attached image" />
          </div>
        </div>
      );

    case "assistant": {
      const modelLabel = turnModelLabel(turn.model, turn.effort);
      return (
        <div className="group" data-msg-uuids={turn.uuids.join(" ")}>
          <div className="flex items-baseline gap-2 mb-1">
            <span className="text-xs tabular-nums text-[var(--color-text-dim)]" title={tsTitle}>
              {ts}
            </span>
            <span className="font-medium text-[var(--color-assistant)]">
              assistant
            </span>
            {modelLabel && (
              <span className="text-xs text-[var(--color-text-dim)]">
                {modelLabel}
              </span>
            )}
            {bookmarked && (
              <span className="text-[var(--color-accent)] text-xs" title="Bookmarked" data-capture-skip>⚑</span>
            )}
            <TurnActions
              onFork={() => onFork(turn.fork_uuid)}
              bookmarked={bookmarked}
              onToggleBookmark={() => onToggleBookmark(turn.uuid)}
              messageId={turn.uuid}
              forkPending={forkPending}
              imageCopy={imageCopy}
              onCopyImage={() => onCopyImage(turn.uuid)}
            />
          </div>
          <div className="pl-4">
            {turn.blocks.map((block, j) => (
              <BlockRenderer
                key={j}
                block={block}
                turnIndex={turnIndex}
                blockIndex={j}
                forceExpand={revealedKeys.has(`${turnIndex}:${j}`)}
                mode={mode}
                modeSeq={modeSeq}
              />
            ))}
          </div>
        </div>
      );
    }

    case "system":
      return (
        <div className="text-[var(--color-system)] text-sm opacity-70">
          <span className="text-xs" title={tsTitle}>
            {ts}
          </span>{" "}
          system:{" "}
          <span data-seg-id={makeSegId(turnIndex, null, "system")}>{turn.content}</span>
        </div>
      );

    // Also the user's input, so it sits on the same side — otherwise the "mine
    // is on the right" rule breaks every time a slash command appears.
    case "command":
      return (
        <div
          className="flex items-baseline gap-2 justify-end"
          data-msg-uuids={turnUuids(turn).join(" ")}
        >
          <span className="text-xs text-[var(--color-text-dim)]" title={tsTitle}>
            {ts}
          </span>
          <span className="text-[var(--color-accent)]">›</span>
          <code
            data-seg-id={makeSegId(turnIndex, null, "command")}
            className="text-sm font-medium text-[var(--color-accent)] break-all"
          >
            {turn.command}
          </code>
        </div>
      );

    // A `!` bash-mode block: the command the user ran and its output. Right
    // aligned like any user turn — running `!cmd` IS the user's input — but
    // styled as a terminal rather than prose.
    case "bash":
      return (
        <div className="flex flex-col items-end" data-msg-uuids={turnUuids(turn).join(" ")}>
          {/* Mirrors the user header: label at the right edge, time before it. */}
          <div className="flex items-baseline gap-2 mb-1 justify-end">
            <span className="text-xs text-[var(--color-text-dim)]" title={tsTitle}>
              {ts}
            </span>
            <span className="font-medium text-[var(--color-tool)]">shell</span>
          </div>
          <div className="max-w-[75%] min-w-0 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm">
            {turn.command !== null && (
              <div className="flex items-baseline gap-2">
                <span className="text-[var(--color-tool)] shrink-0">$</span>
                <code className="min-w-0 font-medium text-[var(--color-tool)] wrap-anywhere">
                  {turn.command}
                </code>
              </div>
            )}
            {(turn.stdout || turn.stderr) && (
              <ShellOutput
                stdout={turn.stdout}
                stderr={turn.stderr}
                afterCommand={turn.command !== null}
                mode={mode}
                modeSeq={modeSeq}
              />
            )}
          </div>
        </div>
      );

    case "summary":
      return (
        <SummaryBlock
          turnIndex={turnIndex}
          content={turn.content}
          mode={mode}
          modeSeq={modeSeq}
          forceExpand={revealedKeys.has(`${turnIndex}:summary`)}
        />
      );

    case "task_notification":
      return (
        <TaskNotificationBlock
          uuids={turn.uuid}
          turnIndex={turnIndex}
          status={turn.status}
          summary={turn.summary}
          result={turn.result}
          forceExpand={revealedKeys.has(`${turnIndex}:task`)}
          mode={mode}
          modeSeq={modeSeq}
        />
      );

    // Why a session stopped. Claude Code records API failures as synthetic
    // assistant lines, which the transcript otherwise hides — leaving a session
    // that died on "Prompt is too long" with no visible explanation.
    case "api_error":
      return (
        <div className="flex items-baseline gap-2 text-sm">
          <span className="text-xs text-[var(--color-text-dim)]" title={tsTitle}>
            {ts}
          </span>
          <span className="text-[var(--color-error)] shrink-0">⚠</span>
          <span className="min-w-0 text-[var(--color-error)] wrap-anywhere">{turn.text}</span>
        </div>
      );

    case "compaction":
      return (
        <div className="flex items-center gap-3 my-5">
          <div className="flex-1 border-t border-[var(--color-border)]" />
          <span className="text-[10px] uppercase tracking-wider text-[var(--color-text-dim)] whitespace-nowrap">
            context compacted · {turn.messages_before} messages above
          </span>
          <div className="flex-1 border-t border-[var(--color-border)]" />
        </div>
      );
  }
});

function TurnActions({
  onFork,
  bookmarked,
  onToggleBookmark,
  messageId,
  forkPending,
  imageCopy,
  onCopyImage,
}: {
  onFork: () => void;
  bookmarked: boolean;
  onToggleBookmark: () => void;
  messageId: string;
  forkPending: boolean;
  imageCopy: ImageCopy | null;
  onCopyImage: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const btn =
    "inline-flex items-center text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-50";
  const busy = forkPending || imageCopy?.state === "busy";

  return (
    // `invisible` alongside `opacity-0`: with opacity alone, WKWebView left the
    // text button's pixels painted after the row un-hovered (a trail of ghost
    // "copy id" buttons down the transcript — the ⋯ escaped it only because
    // Dropdown wraps it in a positioned div, which repaints). visibility is not
    // transitioned, so it flips at once and forces the region to invalidate;
    // the fade-in still animates. It also makes the hidden buttons unclickable.
    <span
      data-capture-skip
      className={`${imageCopy ? "visible opacity-100" : "invisible opacity-0 group-hover:visible group-hover:opacity-100"} transition-opacity flex gap-2 ml-2 items-center`}
    >
      {/* Fork / Bookmark live behind a ⋯ menu so they're not fat-fingered. Both
          resolve by uuid and work on any turn, including pre-compaction ones.
          (Edit / Delete were removed pending a reimplementation.) */}
      <Dropdown
        trigger={busy ? <Spinner /> : <Ellipsis size={13} />}
        title="Message actions"
        disabled={busy}
        triggerClassName={btn}
      >
        {(close) => (
          <>
            <button
              role="menuitem"
              className={menuItemClass}
              disabled={busy}
              title="Create a new session ending at this message — a verbatim copy of the file up to here, keeping pre-compaction history"
              onClick={() => {
                close();
                onFork();
              }}
            >
              Fork from here
            </button>
            <button
              role="menuitem"
              className={menuItemClass}
              title="Flag this message; find it via the header ⚑ list or the sidebar bookmark browser"
              onClick={() => {
                close();
                onToggleBookmark();
              }}
            >
              {bookmarked ? "Remove bookmark" : "Bookmark"}
            </button>
            <button
              role="menuitem"
              className={menuItemClass}
              title="Copy this message to the clipboard as a PNG, as it is shown"
              onClick={() => {
                close();
                onCopyImage();
              }}
            >
              Copy as image
            </button>
          </>
        )}
      </Dropdown>
      {imageCopy?.state === "done" && <span className="text-[10px] text-[var(--color-text-dim)]">image copied ✓</span>}
      {imageCopy?.state === "error" && (
        <span className="text-[10px] text-[var(--color-error)]" title={imageCopy.error}>
          copy failed
        </span>
      )}
      {messageId && (
        <button
          onClick={() => {
            navigator.clipboard.writeText(messageId).then(
              () => {
                setCopied(true);
                setTimeout(() => setCopied(false), 1200);
              },
              () => {}
            );
          }}
          className={btn}
          title={`Copy message id: ${messageId}`}
        >
          {copied ? "copied ✓" : "copy id"}
        </button>
      )}
    </span>
  );
}

// Markdown parsing (react-markdown → AST) is the single most expensive thing in
// a render. Memoizing on the source text means each block is parsed once and is
// NOT re-parsed when the transcript re-renders for an unrelated reason (e.g. a
// find keystroke). `MD_COMPONENTS` is module-level so its identity is stable.
const MD_COMPONENTS = { a: ExternalLink, ...MD_IMAGE_COMPONENTS };
// GFM adds tables, strikethrough, task lists, and autolinks (react-markdown is
// CommonMark-only by default). Module-level for a stable identity (memoization).
// remark-breaks: a single newline renders as a line break (chat convention —
// the CC terminal, claude.ai, GitHub comments, Discord all do this). Strict
// CommonMark treats it as a soft break (a space), which glued "**3. Heading**\n
// text" onto one line.
const MD_REMARK = [remarkGfm, remarkBreaks];
// Syntax highlighting for fenced code blocks (highlight.js via rehype).
// `detect` off: only explicitly-tagged fences highlight — auto-detection on
// arbitrary tool output guesses wrong more than it helps. Colors come from the
// --syn-* theme vars (see index.css), so both themes are covered.
const MD_REHYPE = [[rehypeHighlight, { detect: false }] as unknown as import("unified").Pluggable];

// An Explanatory-style insight, rendered like the terminal: an accent "★ Insight"
// header rule + the points as markdown + a closing rule (CSS borders fill the
// width crisply, unlike literal ─ characters which would wrap).
function InsightBlock({ content }: { content: string }) {
  return (
    <div className="my-3">
      <div className="flex items-center gap-2 text-xs font-semibold text-[var(--color-accent)]">
        <span className="whitespace-nowrap">★ Insight</span>
        <span className="flex-1 border-t border-[var(--color-accent)] opacity-50" />
      </div>
      <div className="py-1">
        <Markdown remarkPlugins={MD_REMARK} rehypePlugins={MD_REHYPE} components={MD_COMPONENTS} urlTransform={mdUrlTransform}>{content}</Markdown>
      </div>
      <div className="border-t border-[var(--color-accent)] opacity-50" />
    </div>
  );
}

const MarkdownBlock = memo(function MarkdownBlock({ text }: { text: string }) {
  const segs = splitInsights(text);
  // Fast path: no insight block → one Markdown render (unchanged behavior).
  if (segs.length === 1 && segs[0].kind === "md") {
    return <Markdown remarkPlugins={MD_REMARK} rehypePlugins={MD_REHYPE} components={MD_COMPONENTS} urlTransform={mdUrlTransform}>{text}</Markdown>;
  }
  return (
    <>
      {segs.map((s, i) =>
        s.kind === "insight" ? (
          <InsightBlock key={i} content={s.content} />
        ) : (
          <Markdown key={i} remarkPlugins={MD_REMARK} rehypePlugins={MD_REHYPE} components={MD_COMPONENTS} urlTransform={mdUrlTransform}>
            {s.text}
          </Markdown>
        ),
      )}
    </>
  );
});

const TaskNotificationBlock = memo(function TaskNotificationBlock({
  uuids,
  turnIndex,
  status,
  summary,
  result,
  forceExpand,
  mode,
  modeSeq,
}: {
  /** Its event uuid — a global-find hit lands on this turn by it. */
  uuids: string;
  turnIndex: number;
  status: string;
  summary: string;
  result: string;
  forceExpand?: boolean;
  mode: AppearanceMode;
  modeSeq: number;
}) {
  const part = mode.taskNotifications;
  // Kept with the switch it was clicked after; any later switch drops it.
  const [clicked, setClicked] = useState<{ seq: number; open: boolean } | null>(null);
  const expanded = clicked?.seq === modeSeq ? clicked.open : part.show === "open";
  const open = expanded || !!forceExpand;
  const lines = useMemo(() => (part.height.kind === "clamp" ? lineCount(ansiText(result)) : 0), [part.height.kind, result]);
  return (
    <div className="my-1 text-sm" data-msg-uuids={uuids}>
      <button
        onClick={() => setClicked({ seq: modeSeq, open: !expanded })}
        className="flex items-baseline text-left w-full hover:bg-[var(--color-surface-hover)] rounded px-2 py-1 transition-colors text-[var(--color-text-dim)]"
      >
        <span className="w-4 shrink-0 text-center">{open ? "▾" : "▸"}</span>
        <span className="shrink-0 mr-1.5">task · {status} ·</span>
        <span data-seg-id={makeSegId(turnIndex, null, "task-summary")} className="truncate min-w-0">
          <AnsiText text={summary} />
        </span>
      </button>
      {open && result && (
        <BodyBox
          height={part.height}
          scrollPx={SCROLL_PX.taskNotifications}
          pad={PY_2}
          lines={lines}
          modeSeq={modeSeq}
          forceFull={forceExpand}
        >
          {(style, ref) => (
            <pre
              ref={ref}
              data-seg-id={makeSegId(turnIndex, null, "task-result")}
              className="ml-4 px-3 py-2 rounded text-xs whitespace-pre-wrap break-words overflow-hidden border-l-2 border-l-[var(--color-border)] bg-[var(--color-tool-result-bg)] text-[var(--color-text-dim)]"
              style={style}
            >
              <AnsiText text={result} />
            </pre>
          )}
        </BodyBox>
      )}
    </div>
  );
});

function SummaryBlock({
  turnIndex,
  content,
  mode,
  modeSeq,
  forceExpand,
}: {
  turnIndex: number;
  content: string;
  mode: AppearanceMode;
  modeSeq: number;
  forceExpand: boolean;
}) {
  const height = mode.compactionSummary.height;
  const lines = useMemo(() => (height.kind === "clamp" ? lineCount(content) : 0), [height.kind, content]);
  return (
    <div className="border-t border-b border-[var(--color-border)] py-2 my-2 text-xs text-[var(--color-text-dim)] italic">
      ── compaction summary ──
      <BodyBox
        height={height}
        scrollPx={SCROLL_PX.compactionSummary}
        lines={lines}
        modeSeq={modeSeq}
        forceFull={forceExpand}
        fade="var(--color-bg)"
      >
        {(style, ref) => (
          <div
            ref={ref}
            data-seg-id={makeSegId(turnIndex, null, "summary")}
            className="mt-1 whitespace-pre-wrap break-words"
            style={style}
          >
            {content}
          </div>
        )}
      </BodyBox>
    </div>
  );
}

/** The `border-t pt-1.5` under the command; a max-height counts the border too. */
const SHELL_PAD: Pad = { top: "calc(0.375rem + 1px)", bottom: "0px" };

function ShellOutput({
  stdout,
  stderr,
  afterCommand,
  mode,
  modeSeq,
}: {
  stdout: string;
  stderr: string;
  afterCommand: boolean;
  mode: AppearanceMode;
  modeSeq: number;
}) {
  const height = mode.shellOutput;
  const lines = useMemo(
    () => (height.kind === "clamp" ? (stdout ? lineCount(stdout) : 0) + (stderr ? lineCount(stderr) : 0) : 0),
    [height.kind, stdout, stderr],
  );
  return (
    <BodyBox
      height={height}
      scrollPx={SCROLL_PX.shellOutput}
      pad={afterCommand ? SHELL_PAD : undefined}
      lines={lines}
      modeSeq={modeSeq}
      fade="var(--color-surface)"
    >
      {(style, ref) => (
        <div
          ref={ref}
          className={`text-xs ${afterCommand ? "mt-1.5 border-t border-[var(--color-border)] pt-1.5" : ""}`}
          style={style}
        >
          {stdout && (
            <pre className="whitespace-pre-wrap wrap-anywhere text-[var(--color-text-dim)]">
              <AnsiText text={stdout} />
            </pre>
          )}
          {stderr && (
            <pre className="whitespace-pre-wrap wrap-anywhere text-[var(--color-error)]">
              <AnsiText text={stderr} />
            </pre>
          )}
        </div>
      )}
    </BodyBox>
  );
}

const BlockRenderer = memo(function BlockRenderer({
  block,
  turnIndex,
  blockIndex,
  forceExpand,
  mode,
  modeSeq,
}: {
  block: RenderedBlock;
  turnIndex: number;
  blockIndex: number;
  forceExpand: boolean;
  mode: AppearanceMode;
  modeSeq: number;
}) {
  switch (block.type) {
    case "text":
      return (
        <div
          data-seg-id={makeSegId(turnIndex, blockIndex, "assistant-text")}
          className="prose prose-sm max-w-none mb-2"
        >
          <MarkdownBlock text={block.text} />
        </div>
      );

    case "thinking":
      return null;

    case "narration":
      if (mode.narration === "hidden") return null;
      return (
        <div
          data-seg-id={makeSegId(turnIndex, blockIndex, "narration")}
          className="prose prose-sm max-w-none mb-2 [&_p]:inline"
        >
          <MarkdownBlock text={block.text} />
          <span className="text-[11px] text-[var(--color-thinking)]"> · summarized</span>
        </div>
      );

    case "tool_call":
      if (mode.toolCalls === "hidden") return null;
      return (
        <ToolCall
          block={block}
          turnIndex={turnIndex}
          blockIndex={blockIndex}
          forceExpand={forceExpand}
          mode={mode}
          modeSeq={modeSeq}
        />
      );
  }
});

function ExternalLink({
  href,
  children,
}: {
  href?: string;
  children?: React.ReactNode;
}) {
  return (
    <a
      href={href}
      onClick={(e) => {
        e.preventDefault();
        if (href) openUrl(href).catch((err) => console.error("openUrl failed:", err));
      }}
    >
      {children}
    </a>
  );
}

function formatTime(ts: string): string {
  try {
    const d = new Date(ts);
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  } catch {
    return "--:--";
  }
}

/** Full local date + time for the timestamp hover tooltip (the row shows only
 *  HH:MM). `undefined` when unparseable, so no misleading tooltip is attached. */
function formatFullDate(ts: string): string | undefined {
  const d = new Date(ts);
  if (isNaN(d.getTime())) return undefined;
  return d.toLocaleString(undefined, { dateStyle: "full", timeStyle: "medium" });
}

function shortenPath(p: string): string {
  const home = p.replace(/^\/Users\/[^/]+/, "~");
  return home;
}

function formatDuration(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

function compact(n: number): string {
  if (n < 1000) return `${n}`;
  if (n < 1_000_000) return `${Math.floor(n / 1000)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}
