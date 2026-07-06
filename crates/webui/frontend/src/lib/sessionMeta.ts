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
