// Claude-manager-local session organization: global colored tags + per-session
// alias/tags/note. Mirrors the Rust `session_meta` store; every mutator is pure
// (returns a new SessionMeta) and prunes entries with no remaining fields.

export interface Tag {
  name: string;
  color: string;
}

export interface SessionEntry {
  alias?: string | null;
  tags?: string[];
  note?: string | null;
}

export interface SessionMeta {
  version: number;
  tags: Tag[];
  sessions: Record<string, SessionEntry>;
}

export const DEFAULT_META: SessionMeta = { version: 1, tags: [], sessions: {} };

/** GitHub-label-ish colors that read on both themes (chips carry explicit color). */
export const TAG_PALETTE = [
  "#e5534b", // red
  "#f0883e", // orange
  "#d4a72c", // yellow
  "#57ab5a", // green
  "#39c5cf", // cyan
  "#539bf5", // blue
  "#986ee2", // purple
  "#e275ad", // pink
];

const FALLBACK_COLOR = "#8b949e"; // a tag name missing from the registry self-heals

export function tagColor(meta: SessionMeta, name: string): string {
  return meta.tags.find((t) => t.name === name)?.color ?? FALLBACK_COLOR;
}

export function entryTags(entry: SessionEntry | undefined): string[] {
  return entry?.tags ?? [];
}

/**
 * The name the sidebar shows for a session: its local alias if set, else
 * claude's auto-generated title. Sorting and text-filtering by "title" must use
 * this so a renamed session orders and matches by what the user actually sees.
 * A blank/whitespace alias falls back to claude's title.
 */
export function sessionTitle(
  meta: SessionMeta,
  id: string,
  claudeTitle: string,
): string {
  return meta.sessions[id]?.alias?.trim() || claudeTitle;
}

/** Sidebar tag filter: empty selection matches all; else ANY-of semantics. */
export function matchesTagFilter(
  entry: SessionEntry | undefined,
  selected: Set<string>,
): boolean {
  if (selected.size === 0) return true;
  return entryTags(entry).some((t) => selected.has(t));
}

/** Least-used palette color, so new tags spread across the palette. */
export function nextPaletteColor(meta: SessionMeta): string {
  const counts = new Map(TAG_PALETTE.map((c) => [c, 0]));
  for (const t of meta.tags) counts.set(t.color, (counts.get(t.color) ?? 0) + 1);
  let best = TAG_PALETTE[0];
  for (const c of TAG_PALETTE) {
    if ((counts.get(c) ?? 0) < (counts.get(best) ?? 0)) best = c;
  }
  return best;
}

function withEntry(
  meta: SessionMeta,
  id: string,
  patch: (e: SessionEntry) => SessionEntry,
): SessionMeta {
  const cur = meta.sessions[id] ?? {};
  const next = patch({ ...cur });
  const sessions = { ...meta.sessions };
  // Prune: an entry with no alias, no tags, and no note is dropped entirely.
  if (!next.alias && !next.note && (next.tags?.length ?? 0) === 0) {
    delete sessions[id];
  } else {
    // Normalize away empty fields so the stored JSON stays minimal.
    const clean: SessionEntry = {};
    if (next.alias) clean.alias = next.alias;
    if (next.tags && next.tags.length > 0) clean.tags = next.tags;
    if (next.note) clean.note = next.note;
    sessions[id] = clean;
  }
  return { ...meta, sessions };
}

export function setAliasIn(meta: SessionMeta, id: string, alias: string | null): SessionMeta {
  const v = alias?.trim() || null;
  return withEntry(meta, id, (e) => ({ ...e, alias: v }));
}

export function setNoteIn(meta: SessionMeta, id: string, note: string | null): SessionMeta {
  const v = note?.trim() || null;
  return withEntry(meta, id, (e) => ({ ...e, note: v }));
}

export function setSessionTagsIn(meta: SessionMeta, id: string, names: string[]): SessionMeta {
  return withEntry(meta, id, (e) => ({ ...e, tags: names }));
}

/** Create a tag, or recolor it if the name already exists. */
export function upsertTagIn(meta: SessionMeta, name: string, color: string): SessionMeta {
  const tags = meta.tags.some((t) => t.name === name)
    ? meta.tags.map((t) => (t.name === name ? { ...t, color } : t))
    : [...meta.tags, { name, color }];
  return { ...meta, tags };
}

/** Add a tag to a session ATOMICALLY: ensure the registry entry exists (created
 *  with the next palette color) and add the name to the session, in ONE returned
 *  meta. Doing these as two separate persisted writes raced — whichever write
 *  landed last erased the other half (dangling session tags / orphan registry
 *  entries). Registry-ensure runs even if the session already has the name, so
 *  re-adding a dangling tag self-heals it. */
export function addSessionTagIn(meta: SessionMeta, id: string, name: string): SessionMeta {
  const n = name.trim();
  if (!n) return meta;
  let next = meta;
  if (!next.tags.some((t) => t.name === n)) {
    next = upsertTagIn(next, n, nextPaletteColor(next));
  }
  const cur = entryTags(next.sessions[id]);
  if (cur.includes(n)) return next;
  return setSessionTagsIn(next, id, [...cur, n]);
}

/** Heal a store where sessions reference tags absent from the registry (legacy
 *  data from the old tag-write race — such tags rendered with a fallback color
 *  and never appeared in the tag selector). Adds each missing name to the
 *  registry with a spread palette color. Orphan registry tags (used by no
 *  session) are left alone — an unused tag is valid. Returns the SAME object
 *  when nothing is missing, so callers can skip a needless persist. */
export function reconcileMeta(meta: SessionMeta): SessionMeta {
  const registered = new Set(meta.tags.map((t) => t.name));
  const missing: string[] = [];
  const seen = new Set<string>();
  for (const e of Object.values(meta.sessions)) {
    for (const name of e.tags ?? []) {
      if (!registered.has(name) && !seen.has(name)) {
        seen.add(name);
        missing.push(name);
      }
    }
  }
  if (missing.length === 0) return meta;
  let next = meta;
  for (const name of missing) {
    next = upsertTagIn(next, name, nextPaletteColor(next));
  }
  return next;
}

/** Rename a registry tag and update every session that references it. Renaming
 *  onto an EXISTING tag name merges: the old registry entry is dropped (the
 *  target's color wins) and sessions carrying both are deduped. No-op on an
 *  empty/unchanged/unknown name. */
export function renameTagIn(meta: SessionMeta, oldName: string, newName: string): SessionMeta {
  const nn = newName.trim();
  if (!nn || nn === oldName || !meta.tags.some((t) => t.name === oldName)) return meta;
  const targetExists = meta.tags.some((t) => t.name === nn);
  const tags = targetExists
    ? meta.tags.filter((t) => t.name !== oldName)
    : meta.tags.map((t) => (t.name === oldName ? { ...t, name: nn } : t));
  let next: SessionMeta = { ...meta, tags };
  for (const [id, e] of Object.entries(meta.sessions)) {
    if (e.tags?.includes(oldName)) {
      const replaced = [...new Set((e.tags ?? []).map((t) => (t === oldName ? nn : t)))];
      next = setSessionTagsIn(next, id, replaced);
    }
  }
  return next;
}

/** Delete a tag from the registry and strip it from every session (pruning). */
export function removeTagIn(meta: SessionMeta, name: string): SessionMeta {
  let next: SessionMeta = { ...meta, tags: meta.tags.filter((t) => t.name !== name) };
  for (const [id, e] of Object.entries(next.sessions)) {
    if (e.tags?.includes(name)) {
      next = setSessionTagsIn(
        next,
        id,
        (e.tags ?? []).filter((t) => t !== name),
      );
    }
  }
  return next;
}
