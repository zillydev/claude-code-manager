import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X } from "lucide-react";
import type { SessionSummary, ToolPreview } from "../types";
import ToolPreviewLine from "./ToolPreviewLine";
import Spinner from "./Spinner";
import MiniCalendar from "./MiniCalendar";
import TagChip from "./TagChip";
import { useSessionMeta } from "../hooks/useSessionMeta";
import { activeTags, archivedTags, entryTags, sessionTitle, tagColor } from "../lib/sessionMeta";
import { parseSearchQuery, isRunnable, type SearchQuery } from "../lib/searchQuery";
import { splitHighlight } from "../lib/highlight";
import { matchOperators, replaceTrailingToken, suggestTools, trailingToken } from "../lib/searchOptions";

interface Hit {
  session_id: string;
  uuid: string | null;
  role: string;
  ts: number | null;
  snippet: string;
  match_count: number;
  tool: ToolPreview | null;
}

/** Discord-style global find: operators + free text, Enter to run.
 *  Session-level narrowing (in:/tag:/session:/mtime) happens HERE, where the
 *  metadata lives; the backend walks only the candidate files. */
export default function GlobalFind({
  sessions,
  activate,
  onOpen,
}: {
  sessions: SessionSummary[];
  /** Focus request from the ⌘F/⌘⇧F shortcuts: bump `n` to focus; `q` non-null
   *  replaces the input (e.g. `session:<id> `). A nonce because this pane stays
   *  mounted across tab switches — a plain initial value could never re-apply. */
  activate: { q: string | null; n: number };
  /** `term` is the query's free text, for the transcript to find within the message. */
  onOpen: (s: SessionSummary, messageUuid?: string, term?: string) => void;
}) {
  const { meta } = useSessionMeta();
  const [input, setInput] = useState("");
  // Drives the Obsidian-style "Search options" legend under the input.
  const [focused, setFocused] = useState(false);
  // Enter dismisses the legend until the query is edited again — after a run,
  // the results deserve the space; while composing, the legend chains operators.
  const [ranQuery, setRanQuery] = useState(false);
  const [running, setRunning] = useState(false);
  // `term` is the text the results were RUN with — highlights key off it, so
  // editing the input never shifts highlights under old results.
  const [result, setResult] = useState<{ hits: Hit[]; truncated: boolean; term: string } | null>(
    null,
  );
  const [loadingMore, setLoadingMore] = useState(false);
  // The exact request the current results came from — Load more re-issues it
  // with a skip, so later edits to the input can't change what page 2 means.
  const lastReq = useRef<Record<string, unknown> | null>(null);
  const [error, setError] = useState<string | null>(null);
  // tag: autocomplete offers active tags; a "…N archived" chip reveals the rest
  // for the current query only (reset when the token stops being a tag: token).
  const [tagShowArchived, setTagShowArchived] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  // Every tool called in the discovered sessions, most used first; refetched when the session list changes.
  const [tools, setTools] = useState<{ name: string; count: number }[] | null>(null);
  useEffect(() => {
    invoke<{ name: string; count: number }[]>("tool_names").then(setTools, () => setTools([]));
  }, [sessions]);
  useEffect(() => {
    if (activate.n === 0) return;
    if (activate.q !== null) setInput(activate.q);
    inputRef.current?.focus();
  }, [activate.n, activate.q]);

  const q = parseSearchQuery(input);

  // Value autocomplete from memory for the current trailing token.
  const lastToken = trailingToken(input);
  let suggestions: string[] = [];
  const sug = (key: string, values: string[]) => {
    const pfx = lastToken.slice(key.length).toLowerCase();
    return values
      .filter((v) => v.toLowerCase().startsWith(pfx) && v.toLowerCase() !== pfx)
      .slice(0, 6)
      .map((v) => key + (v.includes(" ") ? `"${v}"` : v));
  };
  if (lastToken.startsWith("in:")) {
    suggestions = sug("in:", [...new Set(sessions.map((s) => s.project_label))]);
  } else if (lastToken.startsWith("tag:")) {
    suggestions = sug(
      "tag:",
      (tagShowArchived ? meta.tags : activeTags(meta)).map((t) => t.name),
    );
  } else if (lastToken.startsWith("from:")) {
    suggestions = sug("from:", ["user", "assistant"]);
  } else if (lastToken.startsWith("is:")) {
    suggestions = sug("is:", ["bookmarked"]);
  } else if (lastToken.startsWith("tool:")) {
    suggestions = suggestTools(tools ?? [], lastToken.slice("tool:".length)).map((n) => `tool:${n}`);
  }
  const toolsLoading = lastToken.startsWith("tool:") && tools === null;
  const legendOps = matchOperators(lastToken);
  const isTagToken = lastToken.startsWith("tag:");
  // Archived tags that WOULD match the typed prefix but are hidden — surfaced
  // as a "…N archived" chip so they're one click away without cluttering.
  const hiddenArchivedCount =
    isTagToken && !tagShowArchived
      ? sug("tag:", archivedTags(meta).map((t) => t.name)).length
      : 0;
  // A bare before:/after: token gets the MiniCalendar instead of value chips.
  const dateKey =
    lastToken === "before:" || lastToken === "after:" ? lastToken : null;
  const acceptDate = (value: string) => {
    if (!value) return;
    setInput(input.slice(0, input.length - lastToken.length) + dateKey + value + " ");
    setRanQuery(false);
    inputRef.current?.focus();
  };
  const acceptSuggestion = (v: string) => {
    setInput(input.slice(0, input.length - lastToken.length) + v + " ");
    setRanQuery(false);
    setTagShowArchived(false); // reveal was for that one token
    inputRef.current?.focus();
  };

  const candidates = (query: SearchQuery) =>
    sessions.filter((s) => {
      if (query.project && !s.project_label.toLowerCase().includes(query.project.toLowerCase()))
        return false;
      if (query.session) {
        const v = query.session.toLowerCase();
        const name = sessionTitle(meta, s.id, s.title).toLowerCase();
        // An id (prefix) pins one session exactly — what ⌘F prefills — while a
        // plain word still matches alias/title substrings.
        if (
          !s.id.toLowerCase().startsWith(v) &&
          !name.includes(v) &&
          !s.title.toLowerCase().includes(v)
        )
          return false;
      }
      if (query.tags.length) {
        const have = new Set(entryTags(meta.sessions[s.id]));
        if (!query.tags.every((t) => have.has(t))) return false;
      }
      // Append-only files: one whose last write predates `after` can't contain
      // newer messages.
      if (query.after && s.mtime.secs_since_epoch < query.after) return false;
      if (query.bookmarked && (meta.sessions[s.id]?.bookmarks?.length ?? 0) === 0) return false;
      return true;
    });

  const run = async () => {
    setRanQuery(true);
    const query = parseSearchQuery(input);
    if (query.errors.length) {
      setError(query.errors.join(" · "));
      return;
    }
    if (!isRunnable(query)) {
      setError("Add a search term or an operator (in:, from:, tag:, …)");
      return;
    }
    setError(null);
    setRunning(true);
    let req: Record<string, unknown> | null = null;
    try {
      const files = candidates(query).map((s) => ({
        file_path: s.file_path,
        session_id: s.id,
        only_uuids: query.bookmarked ? (meta.sessions[s.id]?.bookmarks ?? []) : null,
      }));
      req = {
        files,
        filters: {
          text: query.text,
          role: query.role ?? null,
          after: query.after ?? null,
          before: query.before ?? null,
          tool: query.tool ?? null,
          hasLink: query.hasLink,
        },
      };
      lastReq.current = req;
      const res = await invoke<{ hits: Hit[]; truncated: boolean }>("search_messages", req);
      // A newer search, or ✕, has replaced this one: its answer is no longer wanted.
      if (lastReq.current !== req) return;
      setResult({ ...res, term: query.text });
    } catch (e) {
      // `req` is null only for a failure before the request existed, which nothing can have superseded.
      if (req === null || lastReq.current === req) setError(String(e));
    } finally {
      if (req === null || lastReq.current === req) setRunning(false);
    }
  };

  const loadMore = async () => {
    if (!result || !lastReq.current || loadingMore) return;
    setLoadingMore(true);
    const prev = lastReq.current as { files: unknown; filters: Record<string, unknown> };
    try {
      const res = await invoke<{ hits: Hit[]; truncated: boolean }>("search_messages", {
        files: prev.files,
        filters: { ...prev.filters, skip: result.hits.length },
      });
      if (lastReq.current !== prev) return;
      setResult({
        hits: [...result.hits, ...res.hits],
        truncated: res.truncated,
        term: result.term,
      });
    } catch (e) {
      if (lastReq.current === prev) setError(String(e));
    } finally {
      setLoadingMore(false);
    }
  };

  // ✕: back to an empty pane with the cursor in the field, so the options legend offers a fresh start.
  const clear = () => {
    lastReq.current = null;
    setInput("");
    setResult(null);
    setError(null);
    setRanQuery(false);
    setRunning(false);
    inputRef.current?.focus();
  };

  // Discord-style flat order: strictly newest message first across ALL
  // sessions. A session header re-appears whenever consecutive cards come from
  // different sessions — context without sacrificing the true order.
  const byId = new Map(sessions.map((s) => [s.id, s]));
  const ordered = result
    ? [...result.hits]
        .filter((h) => byId.has(h.session_id))
        .sort((a, b) => (b.ts ?? 0) - (a.ts ?? 0))
    : [];

  // "15 Aug, 03:00 AM" — date and time joined with a comma ourselves, because
  // toLocaleString's combined form inserts locale glue ("at") between them.
  const fmt = (ts: number | null) => {
    if (ts === null) return "";
    const d = new Date(ts * 1000);
    const date = d.toLocaleDateString([], { day: "numeric", month: "short" });
    const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    return `${date}, ${time}`;
  };

  // Read-only chips showing the parsed interpretation.
  const chips: string[] = [];
  if (q.project) chips.push(`in: ${q.project}`);
  if (q.session) chips.push(`session: ${q.session}`);
  if (q.role) chips.push(`from: ${q.role}`);
  if (q.after) chips.push(`after: ${new Date(q.after * 1000).toLocaleDateString()}`);
  if (q.before) chips.push(`before: ${new Date(q.before * 1000).toLocaleDateString()}`);
  for (const t of q.tags) chips.push(`tag: ${t}`);
  if (q.bookmarked) chips.push("⚑ bookmarked");
  if (q.tool) chips.push(`tool: ${q.tool}`);
  if (q.hasLink) chips.push("has link");
  if (q.text) chips.push(`"${q.text}"`);

  return (
    <div className="flex flex-col h-full min-h-0">
        <div className="p-2 border-b border-[var(--color-border)]">
          {/* relative: hosts the clear-✕ inside the field's right edge, as in the sessions filter. */}
          <div className="relative">
          <input
            ref={inputRef}
            value={input}
            onChange={(e) => {
              setInput(e.target.value);
              setRanQuery(false);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") run();
            }}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            placeholder="Search all sessions…"
            className="w-full bg-[var(--color-bg)] text-[var(--color-text)] text-xs pl-2 pr-6 py-1 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
          />
          {(input || result || error) && (
            <button
              // mousedown + preventDefault: the field keeps focus, so the legend isn't hidden and re-shown.
              onMouseDown={(e) => {
                e.preventDefault();
                clear();
              }}
              title="Clear search"
              className="absolute right-1 top-1/2 -translate-y-1/2 flex items-center px-1 text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
            >
              <X size={12} strokeWidth={2} />
            </button>
          )}
          </div>
          {/* Between tokens it lists every operator, so operators chain; mid-word it narrows to the
              ones that word could become, and hides for an ordinary word. Enter gives results the space. */}
          {focused && !ranQuery && legendOps.length > 0 && (
            <div className="mt-2 rounded border border-[var(--color-border)] bg-[var(--color-bg)] p-2 text-xs space-y-1">
              <div className="font-medium text-[var(--color-text)]">Search options</div>
              {legendOps.map(([op, desc]) => (
                <button
                  key={op}
                  // mousedown (not click) so the insert lands before the input's
                  // blur would hide this panel.
                  onMouseDown={(e) => {
                    e.preventDefault();
                    setInput((v) => replaceTrailingToken(v, op + (op.endsWith(":") ? "" : " ")));
                    setRanQuery(false);
                    inputRef.current?.focus();
                  }}
                  className="w-full text-left flex gap-2 rounded px-1 py-0.5 hover:bg-[var(--color-surface-hover)]"
                >
                  <code className="shrink-0 text-[var(--color-accent)]">{op}</code>
                  <span className="text-[var(--color-text-dim)]">{desc}</span>
                </button>
              ))}
            </div>
          )}
          {dateKey && <MiniCalendar onPick={acceptDate} />}
          {toolsLoading && (
            <div className="mt-2 text-xs text-[var(--color-text-dim)]">loading tools…</div>
          )}
          {(suggestions.length > 0 || hiddenArchivedCount > 0) && (
            <div className="flex gap-1 mt-2 flex-wrap">
              {(() => {
                const isArchived = (v: string) =>
                  !!(isTagToken &&
                    meta.tags.find((t) => `tag:${t.name}` === v || `tag:"${t.name}"` === v)?.archived);
                const chip = (v: string, dim: boolean) => (
                  <button
                    key={v}
                    onClick={() => acceptSuggestion(v)}
                    className={`text-xs px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-accent)] hover:border-[var(--color-accent)] ${
                      dim ? "opacity-60" : ""
                    }`}
                  >
                    {v}
                  </button>
                );
                const active = suggestions.filter((v) => !isArchived(v));
                const archived = suggestions.filter(isArchived);
                // Archived chips sit after a divider so they don't mix in.
                return (
                  <>
                    {active.map((v) => chip(v, false))}
                    {archived.length > 0 && (
                      <span className="self-center text-[10px] text-[var(--color-text-dim)] px-1">
                        · archived:
                      </span>
                    )}
                    {archived.map((v) => chip(v, true))}
                  </>
                );
              })()}
              {hiddenArchivedCount > 0 && (
                <button
                  onClick={() => setTagShowArchived(true)}
                  className="text-xs px-1.5 py-0.5 rounded border border-dashed border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)]"
                  title="Also offer archived tags for this query"
                >
                  …{hiddenArchivedCount} archived
                </button>
              )}
            </div>
          )}
          {chips.length > 0 && suggestions.length === 0 && (
            <div className="flex gap-1 mt-2 flex-wrap text-[10px] text-[var(--color-text-dim)]">
              {chips.map((c) => (
                <span key={c} className="px-1.5 py-0.5 rounded bg-[var(--color-bg)] border border-[var(--color-border)]">
                  {c}
                </span>
              ))}
              <span className="px-1.5 py-0.5">⏎ to search</span>
            </div>
          )}
          {error && <div className="text-xs text-[var(--color-error)] mt-2">{error}</div>}
        </div>

        <div className="flex-1 overflow-y-auto p-2 space-y-3">
          {running && (
            <div className="flex items-center gap-2 text-xs text-[var(--color-text-dim)]">
              <Spinner /> searching…
            </div>
          )}
          {!running && result && (
            <div className="text-[10px] text-[var(--color-text-dim)]">
              {result.hits.length}
              {result.truncated ? "+" : ""} result
              {result.hits.length === 1 && !result.truncated ? "" : "s"}
            </div>
          )}
          {!running &&
            result &&
            ordered.map((h, i) => {
              const s = byId.get(h.session_id)!;
              const newSession = i === 0 || ordered[i - 1].session_id !== h.session_id;
              return (
              <div key={`${h.uuid ?? h.session_id}-${i}`} className={newSession && i > 0 ? "pt-2" : ""}>
                {newSession && (
                  <div className="flex items-baseline gap-1.5 text-xs mb-1 min-w-0">
                    <span className="text-[var(--color-text-dim)] shrink-0">#</span>
                    <span className="font-semibold text-[var(--color-text)] truncate">
                      {sessionTitle(meta, s.id, s.title)}
                    </span>
                    {/* The session's tags — the same chips as its sidebar row.
                        Chips don't shrink; the title truncates instead. */}
                    {entryTags(meta.sessions[s.id]).length > 0 && (
                      <span className="flex gap-1 shrink-0">
                        {entryTags(meta.sessions[s.id]).map((t) => (
                          <TagChip key={t} name={t} color={tagColor(meta, t)} />
                        ))}
                      </span>
                    )}
                    <span className="ml-auto text-[var(--color-text-dim)] shrink-0">
                      {s.project_label}
                    </span>
                  </div>
                )}
                {(
                  <button
                    key={`${h.uuid ?? i}`}
                    onClick={() => onOpen(s, h.uuid ?? undefined, result.term || undefined)}
                    className="w-full text-left text-xs px-3 py-2 rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] hover:border-[var(--color-accent)]"
                  >
                    <div className="flex items-baseline gap-1.5">
                      <span
                        className={`font-medium ${
                          h.role === "user" ? "text-[var(--color-user)]" : "text-[var(--color-assistant)]"
                        }`}
                      >
                        {h.role}
                      </span>
                      <span className="text-[var(--color-text-dim)]">· {fmt(h.ts)}</span>
                      {h.match_count > 1 && (
                        <span
                          className="ml-auto text-xs tabular-nums text-[var(--color-text-dim)]"
                          title={`${h.match_count} matches in this message (up to 3 excerpted below)`}
                        >
                          ×{h.match_count}
                        </span>
                      )}
                    </div>
                    {/* No CSS clipping at all: any hidden overflow inside a <button>
                        makes WKWebView reserve the full content height while painting
                        the border around the visible part — phantom gaps below each
                        card (line-clamp and max-h both hit it; Chrome renders the
                        identical DOM fine). The snippet is already length-capped by
                        the backend (≤3 excerpt windows), so showing it whole is the fix. */}
                    {/* Same type + rhythm as transcript message bodies (px-3/py-2 card,
                        text-sm) so results read like the conversation, not like chrome. */}
                    <div className="mt-1 text-sm text-[var(--color-text)] whitespace-normal wrap-anywhere">
                      {h.tool ? (
                        <ToolPreviewLine tool={h.tool} />
                      ) : splitHighlight(h.snippet, result.term).map((seg, j) =>
                        seg.hit ? (
                          <mark
                            key={j}
                            className="text-inherit rounded-[2px] px-px"
                            style={{
                              backgroundColor:
                                "color-mix(in srgb, var(--color-system) 35%, transparent)",
                            }}
                          >
                            {seg.text}
                          </mark>
                        ) : (
                          <span key={j}>{seg.text}</span>
                        ),
                      )}
                    </div>
                  </button>
                )}
              </div>
              );
            })}
          {!running && result?.truncated && (
            <button
              onClick={loadMore}
              disabled={loadingMore}
              className="w-full text-center text-xs px-2 py-1.5 rounded-md border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-50"
            >
              {loadingMore ? "Loading…" : "Load more"}
            </button>
          )}
        </div>
    </div>
  );
}
