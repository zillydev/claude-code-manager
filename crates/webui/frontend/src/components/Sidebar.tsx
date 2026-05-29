import { useState, useMemo, useCallback, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SessionSummary, SortField, SortDir, DateRange } from "../types";

interface Props {
  sessions: SessionSummary[];
  loading: boolean;
  activeId: string | null;
  onSelect: (s: SessionSummary) => void;
  onRefresh: () => void;
  onCollapse: () => void;
  onDelete: (s: SessionSummary) => void;
}

export default function Sidebar({
  sessions,
  loading,
  activeId,
  onSelect,
  onRefresh,
  onCollapse,
  onDelete,
}: Props) {
  const [textFilter, setTextFilter] = useState("");
  const [projectFilter, setProjectFilter] = useState<string | null>(null);
  const [cwdFilter, setCwdFilter] = useState<string | null>(null);
  const [dateRange, setDateRange] = useState<DateRange>("all");
  const [sortField, setSortField] = useState<SortField>("updated");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const [searchHits, setSearchHits] = useState<Set<string> | null>(null);
  const [searching, setSearching] = useState(false);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Unique project labels and cwds for dropdown options.
  const projects = useMemo(
    () => [...new Set(sessions.map((s) => s.project_label))].sort(),
    [sessions]
  );
  const cwds = useMemo(
    () =>
      [...new Set(sessions.map((s) => s.cwd).filter(Boolean) as string[])].sort(),
    [sessions]
  );

  // Deep search: debounce 400ms, run backend search, store matching IDs.
  const runSearch = useCallback(
    (q: string) => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
      if (!q.trim()) {
        setSearchHits(null);
        setSearching(false);
        return;
      }
      setSearching(true);
      debounceRef.current = setTimeout(async () => {
        try {
          const ids = await invoke<string[]>("search_transcripts", { query: q });
          setSearchHits(new Set(ids));
        } catch {
          setSearchHits(null);
        } finally {
          setSearching(false);
        }
      }, 400);
    },
    []
  );

  useEffect(() => {
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, []);

  const handleTextChange = (val: string) => {
    setTextFilter(val);
    runSearch(val);
  };

  const now = Math.floor(Date.now() / 1000);

  const filtered = useMemo(() => {
    return sessions.filter((s) => {
      // Project filter
      if (projectFilter && s.project_label !== projectFilter) return false;
      // CWD filter
      if (cwdFilter && s.cwd !== cwdFilter) return false;
      // Date range
      const epoch = s.mtime.secs_since_epoch;
      if (dateRange === "today" && now - epoch > 86400) return false;
      if (dateRange === "week" && now - epoch > 604800) return false;
      if (dateRange === "month" && now - epoch > 2592000) return false;
      // Text filter: match title/project locally, plus deep search hits
      if (textFilter.trim()) {
        const q = textFilter.toLowerCase();
        const localMatch =
          s.project_label.toLowerCase().includes(q) ||
          s.title.toLowerCase().includes(q);
        const deepMatch = searchHits ? searchHits.has(s.id) : false;
        if (!localMatch && !deepMatch) return false;
      }
      return true;
    });
  }, [sessions, projectFilter, cwdFilter, dateRange, textFilter, searchHits, now]);

  const sorted = useMemo(() => {
    const items = [...filtered];
    items.sort((a, b) => {
      let cmp = 0;
      switch (sortField) {
        case "updated":
          cmp = a.mtime.secs_since_epoch - b.mtime.secs_since_epoch;
          break;
        case "project":
          cmp = a.project_label.localeCompare(b.project_label);
          break;
        case "title":
          cmp = a.title.localeCompare(b.title);
          break;
        case "tokens":
          cmp = a.total_tokens - b.total_tokens;
          break;
      }
      return sortDir === "desc" ? -cmp : cmp;
    });
    return items;
  }, [filtered, sortField, sortDir]);

  const toggleSort = (field: SortField) => {
    if (sortField === field) {
      setSortDir((d) => (d === "desc" ? "asc" : "desc"));
    } else {
      setSortField(field);
      setSortDir("desc");
    }
  };

  const arrow = (field: SortField) =>
    sortField === field ? (sortDir === "desc" ? " ↓" : " ↑") : "";

  return (
    <aside className="flex flex-col flex-1 border-r border-[var(--color-border)] bg-[var(--color-surface)] overflow-hidden">
      {/* Search */}
      <div className="p-2 border-b border-[var(--color-border)] space-y-1.5">
        <div className="flex gap-1.5">
          <input
            type="text"
            value={textFilter}
            onChange={(e) => handleTextChange(e.target.value)}
            placeholder="Search sessions…"
            className="flex-1 bg-[var(--color-bg)] text-[var(--color-text)] px-2 py-1 rounded text-xs border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
          />
          <button
            onClick={onRefresh}
            className="text-[var(--color-text-dim)] hover:text-[var(--color-text)] text-xs px-1"
            title="Refresh"
          >
            ↻
          </button>
          <button
            onClick={onCollapse}
            className="text-[var(--color-text-dim)] hover:text-[var(--color-text)] text-xs px-1"
            title="Collapse sidebar"
          >
            ◂
          </button>
        </div>

        {/* Filters row */}
        <div className="flex gap-1.5 flex-wrap">
          <select
            value={projectFilter ?? ""}
            onChange={(e) => setProjectFilter(e.target.value || null)}
            className="bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-1.5 py-1 rounded border border-[var(--color-border)] outline-none"
          >
            <option value="">All projects</option>
            {projects.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>

          <select
            value={cwdFilter ?? ""}
            onChange={(e) => setCwdFilter(e.target.value || null)}
            className="bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-1.5 py-1 rounded border border-[var(--color-border)] outline-none max-w-32 truncate"
          >
            <option value="">All directories</option>
            {cwds.map((c) => (
              <option key={c} value={c}>
                {shortenPath(c)}
              </option>
            ))}
          </select>

          <select
            value={dateRange}
            onChange={(e) => setDateRange(e.target.value as DateRange)}
            className="bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-1.5 py-1 rounded border border-[var(--color-border)] outline-none"
          >
            <option value="all">All time</option>
            <option value="today">Today</option>
            <option value="week">This week</option>
            <option value="month">This month</option>
          </select>
        </div>
      </div>

      {/* Sort bar */}
      <div className="flex text-[10px] border-b border-[var(--color-border)] bg-[var(--color-bg)]">
        {(
          [
            ["project", "Project"],
            ["title", "Title"],
            ["updated", "Updated"],
            ["tokens", "Tokens"],
          ] as [SortField, string][]
        ).map(([field, label]) => (
          <button
            key={field}
            onClick={() => toggleSort(field)}
            className={`flex-1 px-1.5 py-1 text-left hover:text-[var(--color-text)] transition-colors ${
              sortField === field
                ? "text-[var(--color-accent)]"
                : "text-[var(--color-text-dim)]"
            }`}
          >
            {label}
            {arrow(field)}
          </button>
        ))}
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto">
        {loading && sessions.length === 0 ? (
          <div className="p-4 text-[var(--color-text-dim)] text-sm">
            Loading…
          </div>
        ) : sorted.length === 0 ? (
          <div className="p-4 text-[var(--color-text-dim)] text-sm">
            No sessions found
          </div>
        ) : (
          sorted.map((s) => (
            <div
              key={s.id}
              className={`relative flex group border-b border-[var(--color-border)] hover:bg-[var(--color-surface-hover)] transition-colors ${
                activeId === s.id
                  ? "bg-[var(--color-surface-hover)] border-l-2 border-l-[var(--color-accent)]"
                  : ""
              }`}
            >
              {/* Main clickable area */}
              <button
                onClick={() => onSelect(s)}
                className="flex-1 text-left px-3 py-2 min-w-0"
              >
                <div className="flex items-center justify-between">
                  <span className="text-xs font-medium text-[var(--color-accent)] truncate">
                    {s.project_label}
                  </span>
                  <span className="text-[10px] text-[var(--color-text-dim)] ml-2 shrink-0">
                    {relativeTime(s.mtime.secs_since_epoch)}
                  </span>
                </div>
                <div className="text-sm text-[var(--color-text)] truncate mt-0.5">
                  {s.title}
                </div>
                <div className="flex items-center gap-2 text-[10px] text-[var(--color-text-dim)] mt-0.5">
                  <span>{compactTokens(s.total_tokens)}</span>
                  {s.git_branch && (
                    <span className="text-[var(--color-accent)] opacity-70">
                      {s.git_branch}
                    </span>
                  )}
                </div>
              </button>

              {/* Delete affordance */}
              <div className="flex items-center shrink-0 pr-1.5 opacity-0 group-hover:opacity-100 transition-opacity">
                {confirmingId === s.id ? (
                  <span className="flex items-center gap-0.5">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        setConfirmingId(null);
                        onDelete(s);
                      }}
                      className="text-[10px] px-1.5 py-0.5 rounded text-[var(--color-error)] border border-[var(--color-error)] hover:bg-[var(--color-error)] hover:text-[var(--color-bg)] transition-colors"
                      title="Confirm delete"
                    >
                      ✓
                    </button>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        setConfirmingId(null);
                      }}
                      className="text-[10px] px-1.5 py-0.5 rounded text-[var(--color-text-dim)] border border-[var(--color-border)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-hover)] transition-colors"
                      title="Cancel"
                    >
                      ✗
                    </button>
                  </span>
                ) : (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      setConfirmingId(s.id);
                    }}
                    className="text-[10px] px-1 py-0.5 rounded text-[var(--color-text-dim)] hover:text-[var(--color-error)] transition-colors"
                    title="Delete session"
                  >
                    ×
                  </button>
                )}
              </div>
            </div>
          ))
        )}
      </div>

      {/* Footer */}
      <div className="p-1.5 border-t border-[var(--color-border)] text-[10px] text-[var(--color-text-dim)] text-center">
        {sorted.length}/{sessions.length} sessions
        {searching && " · searching…"}
      </div>
    </aside>
  );
}

function relativeTime(epochSecs: number): string {
  const now = Math.floor(Date.now() / 1000);
  const diff = now - epochSecs;
  if (diff < 60) return `${diff}s`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

function compactTokens(n: number): string {
  if (n < 1000) return `${n} tok`;
  if (n < 1_000_000) return `${Math.floor(n / 1000)}k tok`;
  return `${(n / 1_000_000).toFixed(1)}M tok`;
}

function shortenPath(p: string): string {
  return p.replace(/^\/Users\/[^/]+/, "~");
}
