/** Sidebar rows picked with ⌘/⇧-click: which sessions, and the row a ⇧-click extends from. */
export type Selection = { ids: ReadonlySet<string>; anchor: string | null };

export const noSelection: Selection = { ids: new Set(), anchor: null };

/** ⌘-click: add the row, or take it back out; either way it becomes the anchor. The first one also picks
 *  the `open` session, the way Finder keeps the file you clicked before ⌘-clicking another. */
export function toggleRow(sel: Selection, id: string, open: string | null = null): Selection {
  const ids = new Set(sel.ids.size === 0 && open !== null && open !== id ? [open] : sel.ids);
  if (!ids.delete(id)) ids.add(id);
  return { ids, anchor: id };
}

/** ⇧-click: every row from the anchor to `id` in `order`, replacing the last range, as Finder does. Before
 *  anything is picked the `open` session is the anchor; with no anchor in the list it picks `id` alone. */
export function extendTo(sel: Selection, id: string, order: string[], open: string | null = null): Selection {
  const anchor = sel.anchor ?? (sel.ids.size === 0 ? open : null);
  const from = anchor === null ? -1 : order.indexOf(anchor);
  const to = order.indexOf(id);
  if (from < 0 || to < 0) return { ids: new Set([id]), anchor: id };
  const [lo, hi] = from < to ? [from, to] : [to, from];
  return { ids: new Set(order.slice(lo, hi + 1)), anchor };
}

/** Only the rows still shown: a filter that hides a selected row takes it out of what Delete would remove. */
export function pruneTo(sel: Selection, visible: string[]): Selection {
  const shown = new Set(visible);
  const ids = [...sel.ids].filter((id) => shown.has(id));
  const anchor = sel.anchor !== null && shown.has(sel.anchor) ? sel.anchor : null;
  return ids.length === sel.ids.size && anchor === sel.anchor ? sel : { ids: new Set(ids), anchor };
}

/** Where the list should look after rows are deleted: `startId` if it survived, else its closest neighbour
 *  in the order it had `before` — upwards first, so the view lands where the deleted run began. */
export function nearestSurvivor(before: string[], startId: string, after: string[]): string | null {
  const at = before.indexOf(startId);
  if (at < 0) return null;
  const left = new Set(after);
  for (let i = at; i >= 0; i--) if (left.has(before[i])) return before[i];
  for (let i = at + 1; i < before.length; i++) if (left.has(before[i])) return before[i];
  return null;
}
