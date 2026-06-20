import { useState, useRef, useEffect, useCallback, memo, Fragment } from "react";
import Markdown from "react-markdown";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  RenderedSession,
  RenderedTurn,
  RenderedBlock,
  SessionStats,
  ForkRef,
  SessionSummary,
} from "../types";
import ToolCall from "./ToolCall";
import { useEditor } from "../hooks/useEditor";
import CommitResultBanner from "./CommitResultBanner";
import FindBar from "./FindBar";
import ForkMarker from "./ForkMarker";
import { useFind } from "../hooks/useFind";
import { usePersistentState } from "../hooks/usePersistentState";
import { makeSegId } from "../lib/segments";
import { isNearBottom } from "../lib/scroll";

interface Props {
  session: RenderedSession;
  onBack: () => void;
  branchPoints?: Map<string, ForkRef[]>;
  onOpenSession?: (s: SessionSummary) => void;
}

export default function Transcript({ session, onBack, branchPoints, onOpenSession }: Props) {
  const [statsOpen, setStatsOpen] = usePersistentState("cm.detailsOpen", true);
  const [compact, setCompact] = usePersistentState("cm.compact", false);
  const bottomRef = useRef<HTMLDivElement>(null);
  const turnsRef = useRef<HTMLDivElement>(null);
  // Tail-follow state: whether the user is currently pinned to the bottom.
  // Read from scroll events (the user's last position) rather than measured
  // after a turn is appended — appended content grows scrollHeight without
  // firing a scroll, so a post-append measurement would misreport "at bottom".
  const atBottomRef = useRef(true);
  const prevSessionId = useRef<string | null>(null);

  const editor = useEditor();
  const [editingUuid, setEditingUuid] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const sourcePath = session.source_path;

  const find = useFind(turnsRef, session.turns);
  const { openFind, next: findNext, prev: findPrev } = find;

  // Stable handlers so the memoized <Turn>s don't re-render on every find
  // keystroke. useEditor's methods are useCallback([])-stable, and sourcePath
  // is stable, so these identities only change when `draft` does (during an
  // edit) — never while typing in the find box.
  const handleStartEdit = useCallback((uuid: string, text: string) => {
    setEditingUuid(uuid);
    setDraft(text);
  }, []);
  const handleCancelEdit = useCallback(() => setEditingUuid(null), []);
  const handleSaveEdit = useCallback(
    async (uuid: string) => {
      await editor.commitEdit(sourcePath, uuid, draft);
      setEditingUuid(null);
    },
    [editor.commitEdit, sourcePath, draft],
  );
  const handleFork = useCallback(
    (forkUuid: string) => {
      editor.fork(sourcePath, forkUuid);
    },
    [editor.fork, sourcePath],
  );
  const handleDelete = useCallback(
    (uuid: string) => {
      editor.deleteMessage(sourcePath, uuid);
    },
    [editor.deleteMessage, sourcePath],
  );

  // Track whether the user is pinned to the bottom, so live-tail appends only
  // follow when they already are. Programmatic scrolls fire this too, keeping
  // the flag accurate.
  useEffect(() => {
    const container = turnsRef.current;
    if (!container) return;
    const onScroll = () => {
      atBottomRef.current = isNearBottom(container);
    };
    container.addEventListener("scroll", onScroll, { passive: true });
    return () => container.removeEventListener("scroll", onScroll);
  }, []);

  // On opening/switching a session, jump to the latest. On a new turn within
  // the same session (live tail), only follow if the user is at the bottom —
  // otherwise leave their scroll position alone.
  useEffect(() => {
    const isNewSession = prevSessionId.current !== session.id;
    prevSessionId.current = session.id;
    if (isNewSession) {
      atBottomRef.current = true;
      bottomRef.current?.scrollIntoView();
    } else if (atBottomRef.current) {
      bottomRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [session.id, session.turns.length]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        openFind();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "g") {
        e.preventDefault();
        if (e.shiftKey) findPrev();
        else findNext();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openFind, findNext, findPrev]);

  const { stats } = session;

  // Suspend compact while find is open so every match is mounted & countable.
  const visibleTurns = compact && !find.open
    ? filterCompact(session.turns)
    : session.turns;

  return (
    <div className="flex flex-col h-full">
      {/* Header bar */}
      <div className="flex items-center gap-3 px-4 py-2.5 border-b border-[var(--color-border)] bg-[var(--color-surface)] shrink-0">
        <button
          onClick={onBack}
          className="text-[var(--color-text-dim)] hover:text-[var(--color-text)] text-sm"
        >
          ← Back
        </button>
        <span className="font-medium text-[var(--color-accent)]">
          {session.project_label}
        </span>
        <span className="text-[var(--color-text)] font-medium truncate">
          {session.title}
        </span>
        <div className="ml-auto flex gap-2">
          <button
            onClick={() => setCompact(!compact)}
            className={`text-xs px-2 py-1 rounded border ${
              compact
                ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                : "border-[var(--color-border)] text-[var(--color-text-dim)]"
            } hover:border-[var(--color-accent)] transition-colors`}
          >
            {compact ? "Show all" : "Compact"}
          </button>
          <button
            onClick={() => setStatsOpen(!statsOpen)}
            className={`text-xs px-2 py-1 rounded border ${
              statsOpen
                ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                : "border-[var(--color-border)] text-[var(--color-text-dim)]"
            } hover:border-[var(--color-accent)] transition-colors`}
          >
            {statsOpen ? "Hide details" : "Details"}
          </button>
        </div>
      </div>

      {/* Session detail panel */}
      {statsOpen && (
        <StatsPanel
          stats={stats}
          eventCount={session.event_count}
          sessionId={session.id}
        />
      )}

      {/* Turns */}
      <div ref={turnsRef} className="flex-1 overflow-y-auto overflow-x-hidden px-4 py-4 space-y-4">
        {find.open && (
          <div className="sticky top-0 z-20 flex justify-end pointer-events-none">
            <div className="pointer-events-auto">
              <FindBar
                query={find.query}
                setQuery={find.setQuery}
                opts={find.opts}
                setOpts={find.setOpts}
                count={find.count}
                activeIndex={find.activeIndex}
                error={find.error}
                onNext={find.next}
                onPrev={find.prev}
                onGoto={find.goTo}
                onClose={find.close}
              />
            </div>
          </div>
        )}
        {editor.result && (
          <CommitResultBanner result={editor.result} onDismiss={editor.dismiss} />
        )}
        {editor.error && (
          <div className="text-[var(--color-error)] text-xs mb-2">{editor.error}</div>
        )}
        {visibleTurns.map((turn, i) => {
          const editUuid =
            (turn.type === "user" || turn.type === "assistant") && turn.editable
              ? turn.edit_uuid
              : null;
          const editing = editUuid !== null && editingUuid === editUuid;
          const forkUuid =
            turn.type === "user" || turn.type === "assistant" ? turn.fork_uuid : null;
          const ownUuid =
            turn.type === "user" || turn.type === "assistant" ? turn.uuid : null;
          let marks: ForkRef[] | undefined;
          if (branchPoints) {
            if (forkUuid) marks = branchPoints.get(forkUuid);
            if (!marks && ownUuid) marks = branchPoints.get(ownUuid);
          }
          return (
            <Fragment key={i}>
              <Turn
                turn={turn}
                turnIndex={i}
                editUuid={editUuid}
                revealedToolKeys={find.revealedToolKeys}
                editing={editing}
                draft={editing ? draft : ""}
                onStartEdit={handleStartEdit}
                onChangeDraft={setDraft}
                onCancelEdit={handleCancelEdit}
                onSaveEdit={handleSaveEdit}
                onFork={handleFork}
                onDelete={handleDelete}
              />
              {marks && onOpenSession && (
                <ForkMarker refs={marks} onOpen={onOpenSession} />
              )}
            </Fragment>
          );
        })}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}

function filterCompact(turns: RenderedTurn[]): RenderedTurn[] {
  const userIndices: number[] = [];
  for (let i = 0; i < turns.length; i++) {
    if (turns[i].type === "user") userIndices.push(i);
  }
  const visible = new Set<number>();
  // Always keep compaction dividers so the boundary stays marked in compact view.
  for (let i = 0; i < turns.length; i++) {
    if (turns[i].type === "compaction") visible.add(i);
  }
  for (const ui of userIndices) {
    visible.add(ui);
    // Assistant turn just before this user message
    for (let j = ui - 1; j >= 0; j--) {
      if (turns[j].type === "assistant") {
        visible.add(j);
        break;
      }
    }
    // Assistant turn just after this user message
    for (let j = ui + 1; j < turns.length; j++) {
      if (turns[j].type === "assistant") {
        visible.add(j);
        break;
      }
    }
  }
  return turns.filter((_, i) => visible.has(i));
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
              {stats.models.map((m) => (
                <span key={m.model}>
                  <span className="text-[var(--color-assistant)] font-mono">{shortModel(m.model)}</span>
                  <span className="text-[var(--color-text-dim)]"> ×{m.turn_count}</span>
                </span>
              ))}
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
  editUuid,
  revealedToolKeys,
  editing,
  draft,
  onStartEdit,
  onChangeDraft,
  onCancelEdit,
  onSaveEdit,
  onFork,
  onDelete,
}: {
  turn: RenderedTurn;
  turnIndex: number;
  editUuid: string | null;
  revealedToolKeys: Set<string>;
  editing: boolean;
  draft: string;
  onStartEdit: (uuid: string, text: string) => void;
  onChangeDraft: (s: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: (uuid: string) => void;
  onFork: (forkUuid: string) => void;
  onDelete: (uuid: string) => void;
}) {
  const ts = "timestamp" in turn && turn.timestamp ? formatTime(turn.timestamp) : "--:--";

  switch (turn.type) {
    case "user":
      return (
        <div className="group">
          <div className="flex items-baseline gap-2 mb-1">
            <span className="text-xs text-[var(--color-text-dim)]">{ts}</span>
            <span className="font-medium text-[var(--color-user)]">user</span>
            <TurnActions
              onEdit={() => editUuid && onStartEdit(editUuid, turn.text)}
              onFork={() => onFork(turn.fork_uuid)}
              onDelete={() => onDelete(turn.uuid)}
              messageId={turn.uuid}
              canMutate={turn.editable}
            />
          </div>
          {editing ? (
            <EditBox
              draft={draft}
              onChange={onChangeDraft}
              onSave={() => editUuid && onSaveEdit(editUuid)}
              onCancel={onCancelEdit}
            />
          ) : (
            <div
              data-seg-id={makeSegId(turnIndex, null, "user-text")}
              className="pl-4 text-sm whitespace-pre-wrap break-words"
            >
              {turn.text}
            </div>
          )}
        </div>
      );

    case "assistant":
      return (
        <div className="group">
          <div className="flex items-baseline gap-2 mb-1">
            <span className="text-xs text-[var(--color-text-dim)]">{ts}</span>
            <span className="font-medium text-[var(--color-assistant)]">
              assistant
            </span>
            {turn.model && (
              <span className="text-xs text-[var(--color-text-dim)]">
                ({shortModel(turn.model)})
              </span>
            )}
            <TurnActions
              onEdit={() => {
                if (!editUuid) return;
                const firstText = turn.blocks.find((b) => b.type === "text");
                onStartEdit(editUuid, firstText && firstText.type === "text" ? firstText.text : "");
              }}
              onFork={() => onFork(turn.fork_uuid)}
              onDelete={() => onDelete(turn.uuid)}
              messageId={turn.uuid}
              canMutate={turn.editable}
            />
          </div>
          {editing && (
            <EditBox
              draft={draft}
              onChange={onChangeDraft}
              onSave={() => editUuid && onSaveEdit(editUuid)}
              onCancel={onCancelEdit}
            />
          )}
          <div className="pl-4">
            {turn.blocks.map((block, j) => (
              <BlockRenderer
                key={j}
                block={block}
                turnIndex={turnIndex}
                blockIndex={j}
                revealedToolKeys={revealedToolKeys}
              />
            ))}
          </div>
        </div>
      );

    case "system":
      return (
        <div className="text-[var(--color-system)] text-sm opacity-70">
          <span className="text-xs">{ts}</span> system:{" "}
          <span data-seg-id={makeSegId(turnIndex, null, "system")}>{turn.content}</span>
        </div>
      );

    case "summary":
      return (
        <div className="border-t border-b border-[var(--color-border)] py-2 my-2 text-xs text-[var(--color-text-dim)] italic">
          ── compaction summary ──
          <div
            data-seg-id={makeSegId(turnIndex, null, "summary")}
            className="mt-1 whitespace-pre-wrap break-words"
          >
            {turn.content}
          </div>
        </div>
      );

    case "task_notification":
      return (
        <TaskNotificationBlock
          turnIndex={turnIndex}
          status={turn.status}
          summary={turn.summary}
          result={turn.result}
          forceExpand={revealedToolKeys.has(`${turnIndex}:task`)}
        />
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
  onEdit,
  onFork,
  onDelete,
  messageId,
  canMutate,
}: {
  onEdit: () => void;
  onFork: () => void;
  onDelete: () => void;
  messageId: string;
  /// Edit/delete operate on the active path, so they're offered only on the
  /// active segment. Fork (resolves by uuid against the message's own tree) and
  /// copy-id (read-only) are always available — including pre-compaction turns.
  canMutate: boolean;
}) {
  const [copied, setCopied] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const btn =
    "text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]";
  return (
    <span className="opacity-0 group-hover:opacity-100 transition-opacity flex gap-2 ml-2">
      {canMutate && (
        <button onClick={onEdit} className={btn}>
          edit
        </button>
      )}
      <button
        onClick={onFork}
        className={btn}
        title="Create a new session ending at this message"
      >
        fork from here
      </button>
      {canMutate &&
        (confirming ? (
          <span className="flex gap-1 items-center">
            <span className="text-[10px] text-[var(--color-error)]">delete?</span>
            <button
              onClick={() => {
                setConfirming(false);
                onDelete();
              }}
              className="text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-error)] text-[var(--color-error)]"
            >
              yes
            </button>
            <button onClick={() => setConfirming(false)} className={btn}>
              no
            </button>
          </span>
        ) : (
          <button
            onClick={() => setConfirming(true)}
            className={btn}
            title="Delete this message (writes a new session without it)"
          >
            delete
          </button>
        ))}
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

function EditBox({
  draft,
  onChange,
  onSave,
  onCancel,
}: {
  draft: string;
  onChange: (s: string) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="pl-4">
      <textarea
        value={draft}
        onChange={(e) => onChange(e.target.value)}
        className="w-full bg-[var(--color-bg)] text-[var(--color-text)] text-sm p-2 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)] font-mono"
        rows={Math.min(20, Math.max(3, draft.split("\n").length + 1))}
      />
      <div className="flex gap-2 mt-1">
        <button onClick={onSave} className="text-xs px-2 py-1 rounded border border-[var(--color-accent)] text-[var(--color-accent)]">
          Save as new session
        </button>
        <button onClick={onCancel} className="text-xs px-2 py-1 rounded border border-[var(--color-border)] text-[var(--color-text-dim)]">
          Cancel
        </button>
      </div>
    </div>
  );
}

// Markdown parsing (react-markdown → AST) is the single most expensive thing in
// a render. Memoizing on the source text means each block is parsed once and is
// NOT re-parsed when the transcript re-renders for an unrelated reason (e.g. a
// find keystroke). `MD_COMPONENTS` is module-level so its identity is stable.
const MD_COMPONENTS = { a: ExternalLink };

const MarkdownBlock = memo(function MarkdownBlock({ text }: { text: string }) {
  return <Markdown components={MD_COMPONENTS}>{text}</Markdown>;
});

const TaskNotificationBlock = memo(function TaskNotificationBlock({
  turnIndex,
  status,
  summary,
  result,
  forceExpand,
}: {
  turnIndex: number;
  status: string;
  summary: string;
  result: string;
  forceExpand: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const open = expanded || forceExpand;
  return (
    <div className="my-1 text-sm">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-baseline text-left w-full hover:bg-[var(--color-surface-hover)] rounded px-2 py-1 transition-colors text-[var(--color-text-dim)]"
      >
        <span className="w-4 shrink-0 text-center">{open ? "▾" : "▸"}</span>
        <span className="shrink-0 mr-1.5">task · {status} ·</span>
        <span data-seg-id={makeSegId(turnIndex, null, "task-summary")} className="truncate min-w-0">
          {summary}
        </span>
      </button>
      {open && result && (
        <pre
          data-seg-id={makeSegId(turnIndex, null, "task-result")}
          className="ml-4 px-3 py-2 rounded text-xs whitespace-pre-wrap break-words overflow-hidden border-l-2 border-l-[var(--color-border)] bg-[var(--color-tool-result-bg)] text-[var(--color-text-dim)]"
          style={{ maxHeight: "400px", overflowY: "auto" }}
        >
          {result}
        </pre>
      )}
    </div>
  );
});

const BlockRenderer = memo(function BlockRenderer({
  block,
  turnIndex,
  blockIndex,
  revealedToolKeys,
}: {
  block: RenderedBlock;
  turnIndex: number;
  blockIndex: number;
  revealedToolKeys: Set<string>;
}) {
  switch (block.type) {
    case "text":
      return (
        <div
          data-seg-id={makeSegId(turnIndex, blockIndex, "assistant-text")}
          className="prose prose-sm prose-invert max-w-none mb-2"
        >
          <MarkdownBlock text={block.text} />
        </div>
      );

    case "thinking":
      return null;

    case "tool_call":
      return (
        <ToolCall
          block={block}
          turnIndex={turnIndex}
          blockIndex={blockIndex}
          forceExpand={revealedToolKeys.has(`${turnIndex}:${blockIndex}`)}
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

function shortModel(model: string): string {
  return model.replace("claude-", "");
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
