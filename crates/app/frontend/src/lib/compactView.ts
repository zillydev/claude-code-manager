import type { RenderedTurn } from "../types";

/**
 * Compact view: the conversation's skeleton. Every user prompt (and every
 * slash-command invocation) is kept, plus the FIRST and LAST assistant turn of
 * each reply, so the long tool-churn in the middle collapses away.
 *
 * "One reply" is not one turn: claude writes each content block as its own
 * jsonl line, and the renderer starts a new assistant turn whenever an
 * `attachment` or `system` line interrupts, so a single answer routinely spans
 * several turns. The unit that matters here is therefore the RUN of assistant
 * turns between two prompts, not an individual turn.
 */
export function filterCompact(turns: RenderedTurn[]): RenderedTurn[] {
  const visible = new Set<number>();
  // Compaction dividers always stay, so the boundary keeps its marker.
  for (let i = 0; i < turns.length; i++) {
    if (turns[i].type === "compaction") visible.add(i);
  }

  let run: number[] = [];
  const closeRun = () => {
    if (run.length === 0) return;
    visible.add(run[0]);
    visible.add(run[run.length - 1]);
    run = [];
  };

  for (let i = 0; i < turns.length; i++) {
    const t = turns[i].type;
    if (t === "user" || t === "command" || t === "bash") {
      closeRun();
      visible.add(i);
    } else if (t === "task_notification") {
      // Also a user-ROLE event — claude's machinery injects it as a user
      // message — so it ends the reply before it, exactly like a prompt does.
      // A background task that finishes long after the turn is over otherwise
      // welds two replies into one run: the answer that actually closed the
      // turn gets demoted to the middle and hidden, and the throwaway "the
      // killed task was X" note that follows inherits the run's last slot.
      // Kept visible for the same reason it is a boundary: it is what prompted
      // the reply after it, and one dim line is cheaper than a reply that
      // appears out of nowhere.
      closeRun();
      visible.add(i);
    } else if (t === "api_error") {
      // The failure ended the reply, so it closes the run — and it is kept for
      // the same reason a task notification is: it explains what happened to a
      // reply that otherwise just stops mid-thought.
      closeRun();
      visible.add(i);
    } else if (t === "assistant") {
      run.push(i);
    }
  }
  // The reply to the LAST prompt has no following prompt to close it. Without
  // this the final answer of every session is unreachable — it only appears
  // once you send another message.
  closeRun();

  return turns.filter((_, i) => visible.has(i));
}
