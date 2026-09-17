import { useEffect, useRef, useState, type ReactNode } from "react";
import { menuItemClass } from "./Dropdown";
import { headerAt, leavesSection } from "../lib/filterRows";

/** One navigable row. Rows sharing a `section` are a group, headed where it starts. */
export interface MenuItem {
  key: string;
  node: ReactNode;
  /** Shows the ✓ column (multi-select menus keep several selected). */
  selected: boolean;
  /** Called on click or Enter. Closing (or not) is the caller's choice — the
   *  project menu closes on pick, the tag menu stays open to toggle more. */
  onSelect: () => void;
  className?: string;
  title?: string;
  section?: string;
  /** Rule above this row — separates the pinned (selected) block from the rest. */
  divider?: boolean;
}

interface Props {
  /** Rows for the current query; called on every keystroke. */
  items: (query: string) => MenuItem[];
  /** Shown when `items` comes back empty. */
  empty: string;
  close: () => void;
  /** Omit the search box for short fixed lists (arrow keys still work). */
  search?: { placeholder: string };
  /** Actions below the list — not part of arrow navigation. */
  footer?: ReactNode;
}

/**
 * A dropdown body with type-to-filter, a scrolling list, and keyboard
 * navigation: ↑/↓ move (wrapping), Enter picks, Esc closes.
 *
 * Keydown is handled here and stopped from propagating, so the app's ⌘
 * shortcuts don't fire while you're typing a filter — which is also why Esc is
 * handled explicitly rather than left to Dropdown's window listener.
 */
export default function FilterMenu({ items, empty, close, search, footer }: Props) {
  const [query, setQuery] = useState("");
  // The cursor tracks the item's KEY, not its position: lists reorder while
  // open (selecting pins a row to the top), and an index would leave the
  // highlight on whatever slid into that slot.
  const [activeKey, setActiveKey] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const rows = useRef<(HTMLButtonElement | null)[]>([]);

  const list = items(query);
  // Falls back to the first row when the keyed item is gone (filtered out, or
  // never set), so there is always something Enter can pick.
  const keyed = list.findIndex((it) => it.key === activeKey);
  const cursor = keyed >= 0 ? keyed : list.length === 0 ? -1 : 0;

  // The menu mounts only while open, so mount === open: take focus so the
  // arrow keys work without a click first.
  useEffect(() => {
    (search ? inputRef.current : boxRef.current)?.focus();
  }, [search]);
  useEffect(() => {
    setActiveKey(null);
  }, [query]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    e.stopPropagation();
    const n = list.length;
    // Scrolling happens HERE, not in an effect on the cursor: arrow keys never
    // reorder the list, so the target row is already rendered at that index —
    // while selecting DOES reorder, and an effect would drag the viewport to
    // the row's new home every time you toggled something.
    const move = (delta: number) => {
      if (n === 0) return;
      const next = (cursor + delta + n) % n;
      setActiveKey(list[next].key);
      rows.current[next]?.scrollIntoView({ block: "nearest" });
    };
    if (e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (cursor >= 0) list[cursor].onSelect();
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  };

  return (
    <div ref={boxRef} tabIndex={-1} onKeyDown={onKeyDown} className="outline-none">
      {search && (
        <div className="p-1.5 border-b border-[var(--color-border)]">
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={search.placeholder}
            spellCheck={false}
            className="w-full bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-2 py-1 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
          />
        </div>
      )}
      <div className="max-h-64 overflow-y-auto">
        {list.length === 0 && (
          <div className="px-3 py-1.5 text-[11px] text-[var(--color-text-dim)]">{empty}</div>
        )}
        {list.map((it, i) => {
          const header = headerAt(list, i);
          // A rule closes the section above when no header of this row's own already draws one.
          const rule = it.divider || leavesSection(list, i);
          return (
            <div key={it.key}>
              {rule && !header && (
                <div className="my-1 border-t border-[var(--color-border)]" />
              )}
              {header && (
                <div className="mt-1 border-t border-[var(--color-border)] px-3 pt-1.5 pb-0.5 text-[10px] uppercase tracking-wide text-[var(--color-text-dim)]">
                  {header}
                </div>
              )}
              <button
                ref={(el) => {
                  rows.current[i] = el;
                }}
                role="menuitem"
                title={it.title}
                // Hover moves the cursor too, so the keyboard highlight and the
                // pointer never disagree about what Enter would pick.
                onMouseEnter={() => setActiveKey(it.key)}
                onClick={(e) => {
                  e.stopPropagation();
                  it.onSelect();
                }}
                className={`${menuItemClass} flex items-center gap-2 ${it.className ?? ""} ${
                  i === cursor ? "bg-[var(--color-surface-hover)]" : ""
                }`}
              >
                <span className={it.selected ? "" : "opacity-0"}>✓</span>
                <span className="truncate">{it.node}</span>
              </button>
            </div>
          );
        })}
      </div>
      {footer}
    </div>
  );
}
