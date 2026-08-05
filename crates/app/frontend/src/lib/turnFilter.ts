import type { RenderedTurn } from "../types";

/** What the in-session view is narrowed to. A per-view gesture, not persisted. */
export type TurnFilterMode = "all" | "user" | "assistant" | "tools" | "bookmarked";

export const TURN_FILTER_LABELS: Record<TurnFilterMode, string> = {
  all: "All turns",
  user: "User messages",
  assistant: "Assistant replies",
  tools: "Tool calls",
  bookmarked: "⚑ Bookmarked",
};

/**
 * Narrow the transcript to one kind of turn. Compaction dividers always
 * survive, so segment boundaries stay marked (the same rule compact view
 * follows). Runs BEFORE filterCompact in the pipeline, and the find bar is fed
 * the same filtered list — find searches what you see.
 *
 * Returns the SAME array for "all" (and when nothing is dropped), so memoized
 * turns don't re-render when no filter is active.
 */
export function filterTurns(
  turns: RenderedTurn[],
  mode: TurnFilterMode,
  bookmarks: ReadonlySet<string>,
): RenderedTurn[] {
  if (mode === "all") return turns;
  const keep = (t: RenderedTurn): boolean => {
    if (t.type === "compaction") return true;
    switch (mode) {
      case "user":
        return t.type === "user" || t.type === "command";
      case "assistant":
        return t.type === "assistant";
      case "tools":
        return t.type === "assistant" && t.blocks.some((b) => b.type === "tool_call");
      case "bookmarked":
        return (t.type === "user" || t.type === "assistant") && bookmarks.has(t.uuid);
    }
  };
  const out = turns.filter(keep);
  return out.length === turns.length ? turns : out;
}
