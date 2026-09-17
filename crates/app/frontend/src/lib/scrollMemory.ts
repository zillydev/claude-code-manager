/** Where each session was last scrolled in Manager, remembered across restarts and shared by windows. */

/** A position as the message at the top of the view and how many pixels into it the view starts. */
export interface ScrollAnchor {
  uuid: string;
  offset: number;
}

interface SavedPosition extends ScrollAnchor {
  /** When it was saved (epoch ms), so the oldest go first past the cap. */
  at: number;
}

export type Positions = Record<string, SavedPosition>;

export const POSITIONS_KEY = "cm:scrollPositions";
export const POSITIONS_CAP = 300;

export interface TurnBox {
  uuid: string;
  top: number;
  bottom: number;
}

/** The first turn reaching below `viewTop` and how far the view starts into it; `box(i)` must follow document
 *  order. Binary search, so a long transcript costs a handful of measurements. */
export function pickAnchor(count: number, box: (i: number) => TurnBox, viewTop: number): ScrollAnchor | null {
  let lo = 0;
  let hi = count;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (box(mid).bottom > viewTop) hi = mid;
    else lo = mid + 1;
  }
  if (lo === count) return null;
  const b = box(lo);
  return { uuid: b.uuid, offset: viewTop - b.top };
}

export function parsePositions(raw: string | null): Positions {
  let data: unknown;
  try {
    data = JSON.parse(raw ?? "{}");
  } catch {
    return {};
  }
  if (!data || typeof data !== "object" || Array.isArray(data)) return {};
  const out: Positions = {};
  for (const [id, v] of Object.entries(data)) {
    const e = v as Partial<SavedPosition> | null;
    if (e && typeof e.uuid === "string" && Number.isFinite(e.offset) && Number.isFinite(e.at)) {
      out[id] = { uuid: e.uuid, offset: e.offset!, at: e.at! };
    }
  }
  return out;
}

export function withPosition(p: Positions, id: string, anchor: ScrollAnchor, now: number, cap = POSITIONS_CAP): Positions {
  const next: Positions = { ...p, [id]: { uuid: anchor.uuid, offset: anchor.offset, at: now } };
  const ids = Object.keys(next);
  if (ids.length <= cap) return next;
  ids.sort((a, b) => next[a].at - next[b].at);
  for (const old of ids.slice(0, ids.length - cap)) delete next[old];
  return next;
}

/** `p` without `id`'s position; the same object when it had none. */
export function withoutPosition(p: Positions, id: string): Positions {
  if (!(id in p)) return p;
  const next = { ...p };
  delete next[id];
  return next;
}

type KeyValue = Pick<Storage, "getItem" | "setItem">;

function defaultStorage(): KeyValue | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function readAll(storage: KeyValue | null): Positions {
  try {
    return parsePositions(storage?.getItem(POSITIONS_KEY) ?? null);
  } catch {
    return {};
  }
}

export function loadPosition(id: string, storage: KeyValue | null = defaultStorage()): ScrollAnchor | null {
  const p = readAll(storage)[id];
  return p ? { uuid: p.uuid, offset: p.offset } : null;
}

/** Saves `id`'s position, or forgets it when `anchor` is null (the view is at the bottom). */
export function savePosition(
  id: string,
  anchor: ScrollAnchor | null,
  now = Date.now(),
  storage: KeyValue | null = defaultStorage(),
): void {
  if (!storage) return;
  const all = readAll(storage);
  const next = anchor ? withPosition(all, id, anchor, now) : withoutPosition(all, id);
  if (next === all) return;
  try {
    storage.setItem(POSITIONS_KEY, JSON.stringify(next));
  } catch {
    // Storage full or unavailable: the position just isn't remembered.
  }
}
