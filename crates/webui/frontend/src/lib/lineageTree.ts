import type { SessionSummary } from "../types";

export type LineageEdge = "root" | "forked" | "copied";

export interface LineageNode {
  session: SessionSummary;
  edge: LineageEdge;
  /** The parent message uuid this session branched at (null for the root). */
  branchMessageUuid: string | null;
  /** The parent's file path, where that branch message lives (null for root). */
  branchParentFilePath: string | null;
  copiedOverlap: number | null;
  children: LineageNode[];
}

const parentId = (s: SessionSummary): string | null => s.forked_from ?? s.copied_from ?? null;

/** Build the whole fork/copy family of `centerId` as a tree rooted at the
 *  oldest ancestor. Cycle-guarded. Returns null if `centerId` isn't present. */
export function buildLineageTree(
  sessions: SessionSummary[],
  centerId: string,
): LineageNode | null {
  const byId = new Map(sessions.map((s) => [s.id, s]));
  if (!byId.has(centerId)) return null;

  // Walk up to the root.
  let rootId = centerId;
  const seenUp = new Set<string>([centerId]);
  for (;;) {
    const p = parentId(byId.get(rootId)!);
    if (!p || !byId.has(p) || seenUp.has(p)) break;
    seenUp.add(p);
    rootId = p;
  }

  // Index children by parent.
  const childrenOf = new Map<string, SessionSummary[]>();
  for (const s of sessions) {
    const p = parentId(s);
    if (p && byId.has(p)) {
      const arr = childrenOf.get(p);
      if (arr) arr.push(s);
      else childrenOf.set(p, [s]);
    }
  }

  const build = (s: SessionSummary, edge: LineageEdge, visited: Set<string>): LineageNode => {
    visited.add(s.id);
    const pid = parentId(s);
    const node: LineageNode = {
      session: s,
      edge,
      branchMessageUuid:
        edge === "forked"
          ? s.forked_from_message
          : edge === "copied"
            ? s.copied_from_message
            : null,
      branchParentFilePath: edge === "root" || !pid ? null : byId.get(pid)?.file_path ?? null,
      copiedOverlap: edge === "copied" ? s.copied_overlap : null,
      children: [],
    };
    for (const c of childrenOf.get(s.id) ?? []) {
      if (visited.has(c.id)) continue;
      const childEdge: LineageEdge = c.forked_from === s.id ? "forked" : "copied";
      node.children.push(build(c, childEdge, visited));
    }
    return node;
  };

  return build(byId.get(rootId)!, "root", new Set<string>());
}
