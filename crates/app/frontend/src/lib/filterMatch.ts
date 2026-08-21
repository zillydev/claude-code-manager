/** A project as the sidebar's filter menu sees it: a name plus every distinct
 *  cwd sessions under that name were run in (>1 = a collision that expands to
 *  per-directory entries). */
export interface ProjectGroup {
  name: string;
  cwds: string[];
}

/** Case-insensitive substring match, with whitespace trimmed. An empty query
 *  matches everything — the menus show their full list until you type. */
export function matches(haystack: string, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return haystack.toLowerCase().includes(q);
}

/**
 * Filter project groups by `query`, matching the project NAME or any of its
 * directories — with 59 projects, `mono` and `Emergent/mono` should both find
 * the same row. A group survives when its name matches (keeping all its cwds,
 * so a name search doesn't hide directories) or, failing that, with only the
 * cwds that match.
 */
export function matchProjectGroups(groups: ProjectGroup[], query: string): ProjectGroup[] {
  if (!query.trim()) return groups;
  const out: ProjectGroup[] = [];
  for (const g of groups) {
    if (matches(g.name, query)) {
      out.push(g);
      continue;
    }
    const cwds = g.cwds.filter((c) => matches(c, query));
    if (cwds.length > 0) out.push({ name: g.name, cwds });
  }
  return out;
}
