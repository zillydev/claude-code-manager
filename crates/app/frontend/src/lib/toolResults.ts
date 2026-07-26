// Back-fill for the live tail: a slow tool's result arrives in a later tail
// batch than its call, so the already-rendered tool_call block sits at
// "(no result)". The backend emits the batch's tool_results as
// `session-tool-results`; this patches them into the transcript by id.

import type { RenderedTurn } from "../types";

export interface ToolResultUpdate {
  tool_use_id: string;
  text: string;
  is_error: boolean;
}

/**
 * Attach `updates` to any tool_call block that matches by id and has no result
 * yet. Blocks already carrying a result (same-batch pairing) are left alone, so
 * a re-delivered result can never overwrite one. Returns the SAME array when
 * nothing matched, so callers can skip a pointless re-render.
 */
export function patchToolResults(
  turns: RenderedTurn[],
  updates: ToolResultUpdate[],
): RenderedTurn[] {
  if (updates.length === 0) return turns;
  const byId = new Map(updates.map((u) => [u.tool_use_id, u]));
  let changed = false;
  const next = turns.map((t) => {
    if (t.type !== "assistant") return t;
    let turnChanged = false;
    const blocks = t.blocks.map((b) => {
      if (b.type === "tool_call" && !b.result) {
        const u = byId.get(b.id);
        if (u) {
          turnChanged = true;
          return { ...b, result: { text: u.text, is_error: u.is_error } };
        }
      }
      return b;
    });
    if (turnChanged) {
      changed = true;
      return { ...t, blocks };
    }
    return t;
  });
  return changed ? next : turns;
}
