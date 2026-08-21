import { useMemo, useState } from "react";
import { Clock, X } from "lucide-react";
import type { DateRange } from "../types";
import type { SessionMeta } from "../lib/sessionMeta";
import { archivedTags } from "../lib/sessionMeta";
import { matchProjectGroups, matches, type ProjectGroup } from "../lib/filterMatch";
import Dropdown, { menuItemClass } from "./Dropdown";
import FilterMenu, { type MenuItem } from "./FilterMenu";
import TagChip from "./TagChip";

const TIME_LABELS: Record<DateRange, string> = {
  all: "All time",
  today: "Today",
  week: "This week",
  month: "This month",
};

interface Props {
  groups: ProjectGroup[];
  /** "" | `name:<name>` | `dir:<cwd>` — the sidebar's project filter value. */
  project: string;
  setProject: (v: string) => void;
  dateRange: DateRange;
  setDateRange: (v: DateRange) => void;
  tagFilter: Set<string>;
  setTagFilter: (f: (prev: Set<string>) => Set<string>) => void;
  meta: SessionMeta;
  onManageTags: () => void;
  shortenPath: (p: string) => string;
}

/**
 * The sidebar's project / tags / time filters.
 *
 * All three are the same themed `Dropdown` over a `FilterMenu` (type-to-filter,
 * scrolling, ↑/↓/Enter): project and time used to be native `<select>`s, whose
 * popups macOS renders itself, ignoring the app palette. Project and tags are
 * the frequently-used pair and keep full triggers; time is rare, so it shrinks
 * to a clock icon that goes accent when it isn't "All time". Active state shows
 * inside the triggers rather than in a chips row, so the row's height never
 * changes. Menus stretch across the row (`fullWidth`) so they can't spill out
 * of a narrow sidebar.
 */
export default function SessionFilters({
  groups,
  project,
  setProject,
  dateRange,
  setDateRange,
  tagFilter,
  setTagFilter,
  meta,
  onManageTags,
  shortenPath,
}: Props) {
  const [showArchived, setShowArchived] = useState(false);
  const anyActive = project !== "" || dateRange !== "all" || tagFilter.size > 0;

  const projectLabel = useMemo(() => {
    if (project.startsWith("name:")) return project.slice(5);
    if (project.startsWith("dir:")) return shortenPath(project.slice(4));
    return "All projects";
  }, [project, shortenPath]);

  const triggerClass = (active: boolean) =>
    `text-xs px-1.5 py-1 rounded border outline-none max-w-40 truncate ${
      active
        ? "border-[var(--color-accent)] text-[var(--color-accent)]"
        : "border-[var(--color-border)] text-[var(--color-text)] bg-[var(--color-bg)]"
    }`;

  const projectItems = (query: string, close: () => void): MenuItem[] => {
    const out: MenuItem[] = [];
    if (!query.trim()) {
      out.push({
        key: "__all__",
        node: "All projects",
        selected: project === "",
        onSelect: () => {
          setProject("");
          close();
        },
      });
    }
    for (const g of matchProjectGroups(groups, query)) {
      // A name with several directories expands to per-directory rows — the
      // collision rule the old <select> used.
      if (g.cwds.length > 1) {
        g.cwds.forEach((c, i) => {
          out.push({
            key: `dir:${c}`,
            node: shortenPath(c),
            title: c,
            section: i === 0 ? g.name : undefined,
            selected: project === `dir:${c}`,
            onSelect: () => {
              setProject(`dir:${c}`);
              close();
            },
          });
        });
      } else {
        out.push({
          key: `name:${g.name}`,
          node: g.name,
          title: g.cwds[0],
          selected: project === `name:${g.name}`,
          onSelect: () => {
            setProject(`name:${g.name}`);
            close();
          },
        });
      }
    }
    return pinSelected(out);
  };

  const tagItems = (query: string): MenuItem[] => {
    const toggle = (name: string) =>
      setTagFilter((prev) => {
        const next = new Set(prev);
        if (next.has(name)) next.delete(name);
        else next.add(name);
        return next;
      });
    const hit = (t: { name: string }) => matches(t.name, query);
    const row = (t: (typeof meta.tags)[number], section?: string): MenuItem => ({
      key: t.name,
      node: <TagChip name={t.name} color={t.color} />,
      section,
      selected: tagFilter.has(t.name),
      className: t.archived ? "opacity-60" : undefined,
      // Toggling keeps the menu open: tag filtering is any-of, so you usually
      // pick more than one.
      onSelect: () => toggle(t.name),
    });
    // Pinned block: the selected tags, in tag order, archived or not (which is
    // also how a selected archived tag stays reachable without revealing the
    // archived section).
    // Live: selecting pins at once, deselecting unpins at once. FilterMenu
    // keeps its cursor on the item across the reorder.
    const isPinned = (t: { name: string }) => tagFilter.has(t.name);
    const pin = meta.tags.filter((t) => isPinned(t) && hit(t));
    const active = meta.tags.filter((t) => !isPinned(t) && !t.archived && hit(t));
    const archived = meta.tags.filter(
      (t) => !isPinned(t) && t.archived && hit(t) && showArchived,
    );
    return [
      ...pin.map((t) => row(t)),
      ...active.map((t, i) => ({
        ...row(t),
        divider: i === 0 && pin.length > 0,
      })),
      ...archived.map((t, i) => row(t, i === 0 ? "Archived" : undefined)),
    ];
  };

  /** Move the selected row to the top, with a rule under it. Single-select
   *  menus only — see `pinned` for why the tag menu can't sort live. */
  const pinSelected = (items: MenuItem[]): MenuItem[] => {
    const i = items.findIndex((it) => it.selected);
    if (i <= 0) return items;
    const rest = items.filter((_, j) => j !== i);
    if (rest.length === 0) return items;
    // The pinned row leaves its section behind; the next row inherits it.
    const orphanSection = items[i].section;
    if (orphanSection && rest[0]) rest[0] = { ...rest[0], section: orphanSection };
    return [{ ...items[i], section: undefined }, { ...rest[0], divider: true }, ...rest.slice(1)];
  };

  return (
    <>
      <Dropdown
        fullWidth
        trigger={projectLabel}
        title="Filter by project"
        triggerClassName={triggerClass(project !== "")}
      >
        {(close) => (
          <FilterMenu
            close={close}
            search={{ placeholder: "Filter projects…" }}
            empty="No projects match"
            items={(q) => projectItems(q, close)}
          />
        )}
      </Dropdown>

      <Dropdown
        fullWidth
        onOpenChange={(o) => {
          if (o) setShowArchived(false);
        }}
        trigger={tagFilter.size > 0 ? `Tags · ${tagFilter.size}` : "Tags"}
        title="Filter by tag"
        triggerClassName={triggerClass(tagFilter.size > 0)}
      >
        {(close) => (
          <FilterMenu
            close={close}
            search={{ placeholder: "Filter tags…" }}
            empty={meta.tags.length === 0 ? "No tags yet" : "No tags match"}
            items={tagItems}
            footer={
              <>
                {archivedTags(meta).length > 0 && (
                  <button
                    role="menuitem"
                    className={`${menuItemClass} text-[var(--color-text-dim)]`}
                    onClick={(e) => {
                      e.stopPropagation();
                      setShowArchived((v) => !v);
                    }}
                  >
                    {showArchived
                      ? "Hide archived"
                      : `Show archived (${archivedTags(meta).length})`}
                  </button>
                )}
                <button
                  role="menuitem"
                  className={`${menuItemClass} mt-1 border-t border-[var(--color-border)] pt-1.5 text-[var(--color-accent)]`}
                  onClick={(e) => {
                    e.stopPropagation();
                    close();
                    onManageTags();
                  }}
                >
                  Manage tags…
                </button>
              </>
            }
          />
        )}
      </Dropdown>

      {/* Time is the rarely-used one, so it costs an icon rather than a
          full-width control — which is also what stops this row wrapping in a
          narrow sidebar. */}
      <Dropdown
        fullWidth
        trigger={<Clock size={14} strokeWidth={1.75} />}
        title={`Time: ${TIME_LABELS[dateRange]}`}
        triggerClassName={`flex items-center px-1.5 py-1 rounded border ${
          dateRange !== "all"
            ? "border-[var(--color-accent)] text-[var(--color-accent)]"
            : "border-[var(--color-border)] text-[var(--color-text-dim)] bg-[var(--color-bg)]"
        }`}
      >
        {(close) => (
          <FilterMenu
            close={close}
            empty="—"
            items={() =>
              (Object.keys(TIME_LABELS) as DateRange[]).map((k) => ({
                key: k,
                node: TIME_LABELS[k],
                selected: dateRange === k,
                onSelect: () => {
                  setDateRange(k);
                  close();
                },
              }))
            }
          />
        )}
      </Dropdown>

      {anyActive && (
        <button
          onClick={() => {
            setProject("");
            setDateRange("all");
            setTagFilter(() => new Set());
          }}
          title="Clear all filters"
          aria-label="Clear all filters"
          className="flex items-center px-1.5 py-1 rounded text-[var(--color-text-dim)] hover:text-[var(--color-accent)] hover:bg-[var(--color-surface-hover)]"
        >
          <X size={14} strokeWidth={2} />
        </button>
      )}
    </>
  );
}
