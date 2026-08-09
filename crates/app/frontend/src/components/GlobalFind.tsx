import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SessionSummary } from "../types";
import Spinner from "./Spinner";
import MiniCalendar from "./MiniCalendar";
import { useSessionMeta } from "../hooks/useSessionMeta";
import { entryTags, sessionTitle } from "../lib/sessionMeta";
import { parseSearchQuery, isRunnable, type SearchQuery } from "../lib/searchQuery";

interface Hit {
  session_id: string;
  uuid: string | null;
  role: string;
  ts: number | null;
  snippet: string;
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
  onOpen: (s: SessionSummary, messageUuid?: string) => void;
}) {
  const { meta } = useSessionMeta();
  const [input, setInput] = useState("");
  // Drives the Obsidian-style "Search options" legend under the input.
  const [focused, setFocused] = useState(false);
  // Enter dismisses the legend until the query is edited again — after a run,
  // the results deserve the space; while composing, the legend chains operators.
  const [ranQuery, setRanQuery] = useState(false);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<{ hits: Hit[]; truncated: boolean } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (activate.n === 0) return;
    if (activate.q !== null) setInput(activate.q);
    inputRef.current?.focus();
  }, [activate.n, activate.q]);

  const q = parseSearchQuery(input);

  // Value autocomplete from memory for the current trailing token.
  const lastToken = input.split(/\s+/).pop() ?? "";
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
    suggestions = sug("tag:", meta.tags.map((t) => t.name));
  } else if (lastToken.startsWith("from:")) {
    suggestions = sug("from:", ["user", "assistant"]);
  } else if (lastToken.startsWith("is:")) {
    suggestions = sug("is:", ["bookmarked"]);
  }
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
    try {
      const files = candidates(query).map((s) => ({
        file_path: s.file_path,
        session_id: s.id,
        only_uuids: query.bookmarked ? (meta.sessions[s.id]?.bookmarks ?? []) : null,
      }));
      const res = await invoke<{ hits: Hit[]; truncated: boolean }>("search_messages", {
        files,
        text: query.text,
        role: query.role ?? null,
        after: query.after ?? null,
        before: query.before ?? null,
        tool: query.tool ?? null,
        hasLink: query.hasLink,
      });
      setResult(res);
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  // Group hits by session, newest hit first; transcript order within.
  const byId = new Map(sessions.map((s) => [s.id, s]));
  const groups: { s: SessionSummary; hits: Hit[] }[] = [];
  if (result) {
    const m = new Map<string, Hit[]>();
    for (const h of result.hits) {
      const arr = m.get(h.session_id) ?? [];
      arr.push(h);
      m.set(h.session_id, arr);
    }
    for (const [id, hits] of m) {
      const s = byId.get(id);
      if (s) groups.push({ s, hits });
    }
    groups.sort(
      (a, b) => Math.max(...b.hits.map((h) => h.ts ?? 0)) - Math.max(...a.hits.map((h) => h.ts ?? 0)),
    );
  }

  const fmt = (ts: number | null) =>
    ts === null
      ? ""
      : new Date(ts * 1000).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

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
            className="w-full bg-[var(--color-bg)] text-[var(--color-text)] text-xs pl-2 pr-2 py-1 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
          />
          {/* Between tokens (empty input, or a completed token + space) the
              legend returns so operators chain — until Enter, when results
              take the space; editing the query brings it back. */}
          {focused && !ranQuery && (input === "" || /\s$/.test(input)) && (
            <div className="mt-2 rounded border border-[var(--color-border)] bg-[var(--color-bg)] p-2 text-xs space-y-1">
              <div className="font-medium text-[var(--color-text)]">Search options</div>
              {(
                [
                  ["in:", "match project name"],
                  ["from:", "user or assistant messages"],
                  ["before:", "messages before a date/time"],
                  ["after:", "messages after a date/time"],
                  ["tag:", "sessions carrying a tag"],
                  ["is:bookmarked", "bookmarked messages only"],
                  ["tool:", "turns that called a tool (Bash, Write…)"],
                  ["has:link", "messages containing a URL"],
                  ["session:", "one session, by name or id"],
                ] as const
              ).map(([op, desc]) => (
                <button
                  key={op}
                  // mousedown (not click) so the insert lands before the input's
                  // blur would hide this panel.
                  onMouseDown={(e) => {
                    e.preventDefault();
                    setInput((v) => v + op + (op.endsWith(":") ? "" : " "));
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
          {suggestions.length > 0 && (
            <div className="flex gap-1 mt-2 flex-wrap">
              {suggestions.map((v) => (
                <button
                  key={v}
                  onClick={() => acceptSuggestion(v)}
                  className="text-xs px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-accent)] hover:border-[var(--color-accent)]"
                >
                  {v}
                </button>
              ))}
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
          {!running && result && groups.length === 0 && (
            <div className="text-xs text-[var(--color-text-dim)]">No matches.</div>
          )}
          {!running && result?.truncated && (
            <div className="text-xs text-[var(--color-system)]">
              Showing the first 200 matches — narrow the query for the rest.
            </div>
          )}
          {!running &&
            groups.map(({ s, hits }) => (
              <div key={s.id} className="space-y-1">
                <div className="flex items-baseline gap-2 text-xs">
                  <span className="font-medium text-[var(--color-text)] truncate">
                    {sessionTitle(meta, s.id, s.title)}
                  </span>
                  <span className="text-[var(--color-text-dim)] shrink-0">
                    {s.project_label} · {hits.length}
                  </span>
                </div>
                {hits.map((h, i) => (
                  <button
                    key={`${h.uuid ?? i}`}
                    onClick={() => onOpen(s, h.uuid ?? undefined)}
                    className="w-full text-left text-xs px-2 py-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] hover:border-[var(--color-accent)] flex items-baseline gap-2"
                  >
                    <span
                      className={`shrink-0 font-medium ${
                        h.role === "user" ? "text-[var(--color-user)]" : "text-[var(--color-assistant)]"
                      }`}
                    >
                      {h.role}
                    </span>
                    <span className="truncate text-[var(--color-text)]">{h.snippet}</span>
                    <span className="ml-auto shrink-0 text-[var(--color-text-dim)]">{fmt(h.ts)}</span>
                  </button>
                ))}
              </div>
            ))}
        </div>
    </div>
  );
}
