import { useState, useRef, useEffect } from "react";
import Markdown from "react-markdown";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  RenderedSession,
  RenderedTurn,
  RenderedBlock,
  SessionStats,
} from "../types";
import ToolCall from "./ToolCall";
import { useEditor } from "../hooks/useEditor";
import CommitResultBanner from "./CommitResultBanner";

interface Props {
  session: RenderedSession;
  onBack: () => void;
}

export default function Transcript({ session, onBack }: Props) {
  const [statsOpen, setStatsOpen] = useState(true);
  const [compact, setCompact] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  const editor = useEditor();
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const sourcePath = session.source_path;

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [session.turns.length]);

  const { stats } = session;

  const visibleTurns = compact
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
      <div className="flex-1 overflow-y-auto overflow-x-hidden px-4 py-4 space-y-4">
        {editor.result && (
          <CommitResultBanner result={editor.result} onDismiss={editor.dismiss} />
        )}
        {editor.error && (
          <div className="text-[var(--color-error)] text-xs mb-2">{editor.error}</div>
        )}
        {visibleTurns.map((turn, i) => {
          const isMessage = turn.type === "user" || turn.type === "assistant";
          const editIndex = isMessage ? turn.edit_index : null;
          return (
            <Turn
              key={i}
              turn={turn}
              editing={editIndex !== null && editingIndex === editIndex}
              draft={draft}
              onStartEdit={(text) => {
                if (editIndex !== null) {
                  setEditingIndex(editIndex);
                  setDraft(text);
                }
              }}
              onChangeDraft={setDraft}
              onCancelEdit={() => setEditingIndex(null)}
              onSaveEdit={async () => {
                if (editIndex !== null) {
                  await editor.commitEdit(sourcePath, editIndex, draft);
                  setEditingIndex(null);
                }
              }}
              onFork={() => {
                if (turn.type === "user" || turn.type === "assistant") {
                  editor.fork(sourcePath, turn.fork_index);
                }
              }}
            />
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

function Turn({
  turn,
  editing,
  draft,
  onStartEdit,
  onChangeDraft,
  onCancelEdit,
  onSaveEdit,
  onFork,
}: {
  turn: RenderedTurn;
  editing: boolean;
  draft: string;
  onStartEdit: (text: string) => void;
  onChangeDraft: (s: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
  onFork: () => void;
}) {
  const ts = turn.timestamp ? formatTime(turn.timestamp) : "--:--";

  switch (turn.type) {
    case "user":
      return (
        <div className="group">
          <div className="flex items-baseline gap-2 mb-1">
            <span className="text-xs text-[var(--color-text-dim)]">{ts}</span>
            <span className="font-medium text-[var(--color-user)]">user</span>
            <TurnActions onEdit={() => onStartEdit(turn.text)} onFork={onFork} messageId={turn.uuid} />
          </div>
          {editing ? (
            <EditBox draft={draft} onChange={onChangeDraft} onSave={onSaveEdit} onCancel={onCancelEdit} />
          ) : (
            <div className="pl-4 text-sm whitespace-pre-wrap break-words">{turn.text}</div>
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
                const firstText = turn.blocks.find((b) => b.type === "text");
                onStartEdit(firstText && firstText.type === "text" ? firstText.text : "");
              }}
              onFork={onFork}
              messageId={turn.uuid}
            />
          </div>
          {editing && (
            <EditBox draft={draft} onChange={onChangeDraft} onSave={onSaveEdit} onCancel={onCancelEdit} />
          )}
          <div className="pl-4">
            {turn.blocks.map((block, j) => (
              <BlockRenderer key={j} block={block} />
            ))}
          </div>
        </div>
      );

    case "system":
      return (
        <div className="text-[var(--color-system)] text-sm opacity-70">
          <span className="text-xs">{ts}</span> system: {turn.content}
        </div>
      );

    case "summary":
      return (
        <div className="border-t border-b border-[var(--color-border)] py-2 my-2 text-xs text-[var(--color-text-dim)] italic">
          ── compaction summary ──
          <div className="mt-1 whitespace-pre-wrap break-words">{turn.content}</div>
        </div>
      );
  }
}

function TurnActions({
  onEdit,
  onFork,
  messageId,
}: {
  onEdit: () => void;
  onFork: () => void;
  messageId: string;
}) {
  const [copied, setCopied] = useState(false);
  const btn =
    "text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]";
  return (
    <span className="opacity-0 group-hover:opacity-100 transition-opacity flex gap-2 ml-2">
      <button onClick={onEdit} className={btn}>
        edit
      </button>
      <button
        onClick={onFork}
        className={btn}
        title="Create a new session ending at this message"
      >
        fork from here
      </button>
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

function BlockRenderer({ block }: { block: RenderedBlock }) {
  switch (block.type) {
    case "text":
      return (
        <div className="prose prose-sm prose-invert max-w-none mb-2">
          <Markdown components={{ a: ExternalLink }}>{block.text}</Markdown>
        </div>
      );

    case "thinking":
      return null;

    case "tool_call":
      return <ToolCall block={block} />;
  }
}

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
