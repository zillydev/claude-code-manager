import { escapeRegExp, type FindOpts } from "./findMatcher";
import type { FindMatch } from "./findMatches";
import type { RenderedTurn } from "../types";

/** Where the bar starts when a find result opens it: the first match in that turn, else the next after it, else the last.
 *  `matches` must be in transcript order, as useFind keeps them. */
export function startMatchIndex(matches: FindMatch[], turnIndex: number): number {
  const i = matches.findIndex((m) => m.turnIndex >= turnIndex);
  return i >= 0 ? i : Math.max(0, matches.length - 1);
}

/** Global find matched `term` literally; with the saved regex toggle on, escape it so the bar does too. */
export function seedQuery(term: string, opts: FindOpts): string {
  return opts.regex ? escapeRegExp(term) : term;
}

/** The active match once a search (re)computes. A requested start turn wins; a new query or toggle set starts over; anything else (live tail, a mode switch) keeps its place. */
export function activeIndexAfterSearch(
  prev: number,
  all: FindMatch[],
  fresh: boolean,
  startTurn: number | null,
): number {
  if (startTurn !== null) return startMatchIndex(all, startTurn);
  if (fresh || all.length === 0) return 0;
  return Math.min(prev, all.length - 1);
}

/** Whether some turn index now names a different turn than before. A live-tail append keeps every index; a mode
 *  switch or a late tool result replacing a turn object does not, which leaves index-keyed reveals pointing elsewhere. */
export function turnsReindexed(prev: RenderedTurn[], next: RenderedTurn[]): boolean {
  if (prev === next) return false;
  if (next.length < prev.length) return true;
  return prev.some((t, i) => next[i] !== t);
}
