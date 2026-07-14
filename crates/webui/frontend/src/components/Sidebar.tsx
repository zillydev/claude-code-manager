import { useState, useMemo, useCallback, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SessionSummary, SortField, SortDir, DateRange } from "../types";
import Spinner from "./Spinner";
import Dropdown, { menuItemClass, ConfirmMenuItem } from "./Dropdown";
import TagChip from "./TagChip";
import TagEditor from "./TagEditor";
import NoteEditor from "./NoteEditor";
import TagManager from "./TagManager";
import { useSessionMeta } from "../hooks/useSessionMeta";
import {
  entryTags,
  matchesSessionName,
  matchesTagFilter,
  sessionTitle,
  tagColor,
} from "../lib/sessionMeta";
import { deriveStatus } from "../lib/sessionStatus";
import type { LiveActivity } from "../hooks/useActivity";

interface Props {
  sessions: SessionSummary[];
  loading: boolean;
  activeId: string | null;
  activity: Map<string, LiveActivity>;
  lastViewed: Record<string, number>;
  onSelect: (s: SessionSummary) => void;
  onRefresh: () => void;
  onCollapse: () => void;
  onDelete: (s: SessionSummary) => void | Promise<void>;
  onNewSession: (cwd: string) => Promise<void>;
  onViewTree: (s: SessionSummary) => void;
  onOpenSettings: () => void;
}

export default function Sidebar({
  sessions,
  loading,
  activeId,
  activity,
  lastViewed,
  onSelect,
  onRefresh,
  onCollapse,
  onDelete,
  onNewSession,
  onViewTree,
  onOpenSettings,
}: Props) {
  const { meta, setAlias } = useSessionMeta();
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [draftAlias, setDraftAlias] = useState("");
  const [tagsFor, setTagsFor] = useState<SessionSummary | null>(null);
  const [noteFor, setNoteFor] = useState<SessionSummary | null>(null);
  const [manageTags, setManageTags] = useState(false);
  const [textFilter, setTextFilter] = useState("");
  // One project/directory filter: "" | "name:<basename>" | "dir:<cwd>".
  const [filter, setFilter] = useState<string>("");
  const [newOpen, setNewOpen] = useState(false);
  const [newPath, setNewPath] = useState("");
  const [newErr, setNewErr] = useState<string | null>(null);
  const [dateRange, setDateRange] = useState<DateRange>("all");
  const [tagFilter, setTagFilter] = useState<Set<string>>(new Set());
  const [sortField, setSortField] = useState<SortField>("updated");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const [searchHits, setSearchHits] = useState<Set<string> | null>(null);
  const [searching, setSearching] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Fork lineage: look up a session by id, and find children forked from a source.
  const byId = useMemo(
    () => new Map(sessions.map((s) => [s.id, s])),
    [sessions]
  );
  const childrenBySource = useMemo(() => {
    const m = new Map<string, SessionSummary[]>();
    for (const s of sessions) {
      if (s.forked_from) {
        const arr = m.get(s.forked_from);
        if (arr) arr.push(s);
        else m.set(s.forked_from, [s]);
      }
    }
    return m;
  }, [sessions]);

  const copiedChildrenBySource = useMemo(() => {
    const m = new Map<string, SessionSummary[]>();
    for (const s of sessions) {
      if (s.copied_from) {
        const arr = m.get(s.copied_from);
        if (arr) arr.push(s);
        else m.set(s.copied_from, [s]);
      }
    }
    return m;
  }, [sessions]);

  // Rows to accent while hovering a session: its source + its forks/copies.
  const relatedIds = useMemo(() => {
    if (!hoveredId) return null;
    const ids = new Set<string>();
    const h = byId.get(hoveredId);
    if (h?.forked_from) ids.add(h.forked_from);
    if (h?.copied_from) ids.add(h.copied_from);
    for (const c of childrenBySource.get(hoveredId) ?? []) ids.add(c.id);
    for (const c of copiedChildrenBySource.get(hoveredId) ?? []) ids.add(c.id);
    return ids.size ? ids : null;
  }, [hoveredId, byId, childrenBySource, copiedChildrenBySource]);

  // Sessions grouped by project name (basename), each with its distinct cwds.
  // A group with >1 cwd is a "collision" and expands to per-directory options.
  const projectGroups = useMemo(() => {
    const byName = new Map<string, Set<string>>();
    for (const s of sessions) {
      const set = byName.get(s.project_label) ?? new Set<string>();
      if (s.cwd) set.add(s.cwd);
      byName.set(s.project_label, set);
    }
    return [...byName.entries()]
      .map(([name, cwds]) => ({ name, cwds: [...cwds].sort() }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [sessions]);

  // The concrete cwd implied by the current project filter, if any.
  const selectedCwd = useMemo(() => {
    if (filter.startsWith("dir:")) return filter.slice(4);
    if (filter.startsWith("name:")) {
      const g = projectGroups.find((x) => x.name === filter.slice(5));
      return g && g.cwds.length === 1 ? g.cwds[0] : null;
    }
    return null;
  }, [filter, projectGroups]);

  const createSession = async (cwd: string) => {
    const c = cwd.trim();
    if (!c) return;
    setCreating(true);
    try {
      await onNewSession(c);
      setNewOpen(false);
      setNewPath("");
      setNewErr(null);
    } catch (e) {
      setNewErr(String(e));
    } finally {
      setCreating(false);
    }
  };

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
      // Project / directory filter (one selector).
      if (filter.startsWith("name:") && s.project_label !== filter.slice(5)) return false;
      if (filter.startsWith("dir:") && s.cwd !== filter.slice(4)) return false;
      // Tag filter (any-of): only sessions carrying at least one selected tag.
      if (!matchesTagFilter(meta.sessions[s.id], tagFilter)) return false;
      // Date range
      const epoch = s.mtime.secs_since_epoch;
      if (dateRange === "today" && now - epoch > 86400) return false;
      if (dateRange === "week" && now - epoch > 604800) return false;
      if (dateRange === "month" && now - epoch > 2592000) return false;
      // Text filter: match either name the row shows (alias AND claude's) or the
      // project locally, plus deep search hits.
      if (textFilter.trim()) {
        const q = textFilter.toLowerCase();
        const localMatch =
          s.project_label.toLowerCase().includes(q) ||
          matchesSessionName(meta, s.id, s.title, textFilter);
        const deepMatch = searchHits ? searchHits.has(s.id) : false;
        if (!localMatch && !deepMatch) return false;
      }
      return true;
    });
  }, [sessions, filter, dateRange, textFilter, searchHits, now, meta, tagFilter]);

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
          // Sort by the name the row shows: local alias if set, else claude's.
          cmp = sessionTitle(meta, a.id, a.title).localeCompare(
            sessionTitle(meta, b.id, b.title),
          );
          break;
        case "tokens":
          cmp = a.total_tokens - b.total_tokens;
          break;
      }
      return sortDir === "desc" ? -cmp : cmp;
    });
    return items;
  }, [filtered, sortField, sortDir, meta]);

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
          {/* relative: hosts the clear-✕ inside the field's right edge. */}
          <div className="relative flex-1">
            <input
              type="text"
              value={textFilter}
              onChange={(e) => handleTextChange(e.target.value)}
              placeholder="Search sessions…"
              className="w-full bg-[var(--color-bg)] text-[var(--color-text)] pl-2 pr-6 py-1 rounded text-xs border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
            />
            {textFilter && (
              <button
                // Same path as typing: also clears the deep-search hits/spinner.
                onClick={() => handleTextChange("")}
                title="Clear search"
                className="absolute right-1 top-1/2 -translate-y-1/2 text-xs leading-none px-1 text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
              >
                ✕
              </button>
            )}
          </div>
          <button
            onClick={onRefresh}
            disabled={loading}
            className={`text-xs px-1 ${
              loading
                ? "animate-spin text-[var(--color-accent)]"
                : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
            }`}
            title={loading ? "Refreshing…" : "Refresh"}
          >
            ↻
          </button>
          <button
            onClick={onOpenSettings}
            className="text-sm leading-none px-1.5 py-1 rounded text-[var(--color-text-dim)] hover:text-[var(--color-accent)] hover:bg-[var(--color-surface-hover)]"
            title="Settings"
          >
            ⚙
          </button>
          <button
            onClick={onCollapse}
            className="text-base leading-none px-1.5 py-1 rounded text-[var(--color-text-dim)] hover:text-[var(--color-accent)] hover:bg-[var(--color-surface-hover)]"
            title="Collapse sidebar (⌘B)"
          >
            «
          </button>
        </div>

        {/* Filters row */}
        <div className="relative flex gap-1.5 flex-wrap items-center">
          <button
            onClick={() => setNewOpen((o) => !o)}
            className="text-xs px-2 py-1 rounded bg-[var(--color-accent)] text-white"
            title="New session"
          >
            + New
          </button>
          {newOpen && (
            <div className="absolute top-full left-0 mt-1 z-30 w-64 p-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] shadow-lg space-y-2">
              {selectedCwd && (
                <button
                  onClick={() => createSession(selectedCwd)}
                  disabled={creating}
                  className="w-full text-left text-xs px-2 py-1 rounded bg-[var(--color-bg)] border border-[var(--color-border)] hover:border-[var(--color-accent)] truncate disabled:opacity-50"
                  title={selectedCwd}
                >
                  New in {shortenPath(selectedCwd)}
                </button>
              )}
              <div className="flex gap-1">
                <input
                  type="text"
                  value={newPath}
                  onChange={(e) => setNewPath(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") createSession(newPath);
                  }}
                  placeholder="paste a directory path…"
                  className="flex-1 bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-2 py-1 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
                />
                <button
                  onClick={() => createSession(newPath)}
                  disabled={!newPath.trim() || creating}
                  className="text-xs px-2 py-1 rounded bg-[var(--color-accent)] text-white disabled:opacity-40 inline-flex items-center gap-1.5"
                >
                  {creating && <Spinner />}
                  Create
                </button>
              </div>
              {newErr && <div className="text-[var(--color-error)] text-[10px]">{newErr}</div>}
            </div>
          )}
          <select
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            className="bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-1.5 py-1 rounded border border-[var(--color-border)] outline-none max-w-40 truncate"
          >
            <option value="">All projects</option>
            {projectGroups.map((g) =>
              g.cwds.length > 1 ? (
                <optgroup key={g.name} label={g.name}>
                  {g.cwds.map((c) => (
                    <option key={c} value={`dir:${c}`}>
                      {shortenPath(c)}
                    </option>
                  ))}
                </optgroup>
              ) : (
                <option key={g.name} value={`name:${g.name}`}>
                  {g.name}
                </option>
              )
            )}
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

          {meta.tags.length > 0 && (
            <Dropdown
              trigger={tagFilter.size > 0 ? `tags · ${tagFilter.size}` : "tags"}
              title="Filter by tag"
              triggerClassName={`text-xs px-1.5 py-1 rounded border outline-none ${
                tagFilter.size > 0
                  ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                  : "border-[var(--color-border)] text-[var(--color-text)] bg-[var(--color-bg)]"
              }`}
            >
              {(close) => (
                <>
                  {meta.tags.map((t) => {
                    const on = tagFilter.has(t.name);
                    return (
                      <button
                        key={t.name}
                        role="menuitem"
                        className={`${menuItemClass} flex items-center gap-2`}
                        onClick={(e) => {
                          e.stopPropagation();
                          setTagFilter((prev) => {
                            const next = new Set(prev);
                            if (next.has(t.name)) next.delete(t.name);
                            else next.add(t.name);
                            return next;
                          });
                        }}
                      >
                        <span className={on ? "" : "opacity-0"}>✓</span>
                        <TagChip name={t.name} color={t.color} />
                      </button>
                    );
                  })}
                  {tagFilter.size > 0 && (
                    <button
                      role="menuitem"
                      className={`${menuItemClass} text-[var(--color-text-dim)]`}
                      onClick={(e) => {
                        e.stopPropagation();
                        setTagFilter(new Set());
                      }}
                    >
                      Clear filter
                    </button>
                  )}
                  <button
                    role="menuitem"
                    className={`${menuItemClass} mt-1 border-t border-[var(--color-border)] pt-1.5 text-[var(--color-accent)]`}
                    onClick={(e) => {
                      e.stopPropagation();
                      close();
                      setManageTags(true);
                    }}
                  >
                    Manage tags…
                  </button>
                </>
              )}
            </Dropdown>
          )}
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
          <div className="p-4 flex items-center gap-2 text-[var(--color-text-dim)] text-sm">
            <Spinner /> Loading…
          </div>
        ) : sorted.length === 0 ? (
          <div className="p-4 text-[var(--color-text-dim)] text-sm">
            No sessions found
          </div>
        ) : (
          sorted.map((s) => {
            const entry = meta.sessions[s.id];
            const renaming = renamingId === s.id;
            const act = activity.get(s.id);
            const liveEventAt = act?.at ?? s.last_event_at?.secs_since_epoch ?? null;
            const awaiting = act?.awaiting ?? s.awaiting_input;
            // No view history (never opened) ⇒ not unread, else every session floods
            // unread on first launch. Baseline to the session's own last event so only
            // sessions you've opened (which persist a real lastViewed) can go unread.
            const status = deriveStatus(liveEventAt, awaiting, lastViewed[s.id] ?? liveEventAt ?? 0, now);
            // You're looking at the open session — never flag it unread.
            const unread = status.unread && s.id !== activeId;
            return (
            <div
              key={s.id}
              onMouseEnter={() => setHoveredId(s.id)}
              onMouseLeave={() => setHoveredId((h) => (h === s.id ? null : h))}
              className={`relative flex group border-b border-[var(--color-border)] hover:bg-[var(--color-surface-hover)] transition-colors ${
                activeId === s.id
                  ? "bg-[var(--color-surface-hover)] border-l-2 border-l-[var(--color-accent)]"
                  : ""
              } ${
                relatedIds?.has(s.id)
                  ? "ring-1 ring-inset ring-[var(--color-accent)]"
                  : ""
              }`}
            >
              {(status.running || unread) && (
                <span
                  className={`absolute left-1 top-2.5 h-2 w-2 rounded-full ${
                    status.running ? "bg-green-500 animate-pulse" : "bg-[var(--color-accent)]"
                  }`}
                  title={
                    status.running
                      ? "Claude is working in this session"
                      : "Unread — new since you last opened it"
                  }
                />
              )}
              {renaming ? (
                /* Inline alias input (an input can't live inside the row button) */
                <div className="flex-1 px-3 py-2 min-w-0 flex items-center gap-2">
                  <input
                    autoFocus
                    value={draftAlias}
                    onChange={(e) => setDraftAlias(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        setAlias(s.id, draftAlias); // trimmed-empty clears
                        setRenamingId(null);
                      } else if (e.key === "Escape") {
                        e.preventDefault();
                        setRenamingId(null);
                      }
                    }}
                    onBlur={() => setRenamingId(null)}
                    placeholder="Alias (leave empty to clear)"
                    className="w-full bg-[var(--color-bg)] text-[var(--color-text)] text-sm px-1.5 py-1 rounded border border-[var(--color-accent)] outline-none"
                  />
                </div>
              ) : (
              /* Main clickable area */
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
                  {entry?.alias ? (
                    <>
                      <div
                        className={`text-sm text-[var(--color-text)] truncate mt-0.5 ${
                          unread ? "font-semibold" : ""
                        }`}
                      >
                        {entry.alias}
                      </div>
                      <div className="text-[11px] text-[var(--color-text-dim)] truncate">
                        {s.title}
                      </div>
                    </>
                  ) : (
                    <div
                      className={`text-sm text-[var(--color-text)] truncate mt-0.5 ${
                        unread ? "font-semibold" : ""
                      }`}
                    >
                      {s.title}
                    </div>
                  )}
                  <div className="flex items-center gap-2 text-[10px] text-[var(--color-text-dim)] mt-0.5">
                    <span>{compactTokens(s.total_tokens)}</span>
                    {s.git_branch && (
                      <span className="text-[var(--color-accent)] opacity-70">
                        {s.git_branch}
                      </span>
                    )}
                    {childrenBySource.has(s.id) && (
                      <span
                        onClick={(e) => {
                          e.stopPropagation();
                          onViewTree(s);
                        }}
                        className="text-[var(--color-accent)] opacity-80 cursor-pointer hover:opacity-100"
                        title={`${childrenBySource.get(s.id)!.length} session(s) forked from this — view tree`}
                      >
                        ⑂ {childrenBySource.get(s.id)!.length}
                      </span>
                    )}
                    {copiedChildrenBySource.has(s.id) && (
                      <span
                        onClick={(e) => {
                          e.stopPropagation();
                          onViewTree(s);
                        }}
                        className="text-[var(--color-accent)] opacity-80 cursor-pointer hover:opacity-100"
                        title={`${copiedChildrenBySource.get(s.id)!.length} session(s) copied from this — view tree`}
                      >
                        ⧉ {copiedChildrenBySource.get(s.id)!.length}
                      </span>
                    )}
                    {entry?.note && (
                      <span title={entry.note} className="shrink-0">
                        ✎
                      </span>
                    )}
                  </div>
                  {entryTags(entry).length > 0 && (
                    <div className="mt-1 flex flex-wrap gap-1">
                      {entryTags(entry).map((name) => (
                        <TagChip key={name} name={name} color={tagColor(meta, name)} />
                      ))}
                    </div>
                  )}
                  {s.forked_from && (
                    <div className="mt-1">
                      <span
                        onClick={(e) => {
                          e.stopPropagation();
                          const src = byId.get(s.forked_from!);
                          if (src) onSelect(src);
                        }}
                        title={
                          byId.has(s.forked_from)
                            ? "Forked from this session — click to open"
                            : `Forked from ${s.forked_from} (not in list)`
                        }
                        className={`inline-flex max-w-full items-center gap-1 truncate text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] ${
                          byId.has(s.forked_from)
                            ? "hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] cursor-pointer"
                            : "opacity-60"
                        }`}
                      >
                        ⮌ from {byId.get(s.forked_from)?.title ?? s.forked_from.slice(0, 8)}
                      </span>
                    </div>
                  )}
                  {s.copied_from && (
                    <div className="mt-1">
                      <span
                        onClick={(e) => {
                          e.stopPropagation();
                          const src = byId.get(s.copied_from!);
                          if (src) onSelect(src);
                        }}
                        title={
                          byId.has(s.copied_from)
                            ? "History copied from this session — click to open"
                            : `Copied from ${s.copied_from} (not in list)`
                        }
                        className={`inline-flex max-w-full items-center gap-1 truncate text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] ${
                          byId.has(s.copied_from)
                            ? "hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] cursor-pointer"
                            : "opacity-60"
                        }`}
                      >
                        ⮌ copied from {byId.get(s.copied_from)?.title ?? s.copied_from.slice(0, 8)}
                        {s.copied_overlap != null && ` · ${s.copied_overlap} shared`}
                      </span>
                    </div>
                  )}
                </button>
              )}

              {/* Per-row ⋯ menu: Rename / Tags / Note / Delete */}
              {!renaming && (
              <div className={`flex items-center shrink-0 pr-1.5 transition-opacity ${deletingId === s.id ? "opacity-100" : "opacity-0 group-hover:opacity-100"}`}>
                  <Dropdown
                    trigger={deletingId === s.id ? <Spinner /> : "⋯"}
                    title="Session actions"
                    align="right"
                    disabled={deletingId === s.id}
                    triggerClassName="text-[13px] leading-none px-1.5 py-0.5 rounded text-[var(--color-text-dim)] hover:text-[var(--color-accent)] hover:bg-[var(--color-surface-hover)]"
                  >
                    {(close) => (
                      <>
                        <button
                          role="menuitem"
                          className={menuItemClass}
                          onClick={(e) => {
                            e.stopPropagation();
                            close();
                            setDraftAlias(entry?.alias ?? "");
                            setRenamingId(s.id);
                          }}
                        >
                          Rename
                        </button>
                        <button
                          role="menuitem"
                          className={menuItemClass}
                          onClick={(e) => {
                            e.stopPropagation();
                            close();
                            setTagsFor(s);
                          }}
                        >
                          Tags…
                        </button>
                        <button
                          role="menuitem"
                          className={menuItemClass}
                          onClick={(e) => {
                            e.stopPropagation();
                            close();
                            setNoteFor(s);
                          }}
                        >
                          Note…
                        </button>
                        <ConfirmMenuItem
                          label="Delete"
                          confirmLabel="Confirm delete"
                          close={close}
                          onConfirm={async () => {
                            setDeletingId(s.id);
                            try {
                              await onDelete(s);
                            } finally {
                              setDeletingId(null);
                            }
                          }}
                        />
                      </>
                    )}
                  </Dropdown>
                </div>
              )}
            </div>
            );
          })
        )}
      </div>

      {/* Footer */}
      <div className="p-1.5 border-t border-[var(--color-border)] text-[10px] text-[var(--color-text-dim)] text-center">
        {sorted.length}/{sessions.length} sessions
        {searching && (
          <span className="inline-flex items-center gap-1 ml-1">
            · <Spinner /> searching…
          </span>
        )}
      </div>

      {tagsFor && (
        <TagEditor
          sessionId={tagsFor.id}
          title={meta.sessions[tagsFor.id]?.alias ?? tagsFor.title}
          onClose={() => setTagsFor(null)}
        />
      )}
      {noteFor && (
        <NoteEditor
          sessionId={noteFor.id}
          title={meta.sessions[noteFor.id]?.alias ?? noteFor.title}
          onClose={() => setNoteFor(null)}
        />
      )}
      {manageTags && <TagManager onClose={() => setManageTags(false)} />}
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
