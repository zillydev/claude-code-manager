import type { RenderedTurn } from "../types";

/**
 * Where a jump target (a message uuid) actually sits relative to what the
 * transcript is showing.
 *
 * - `visible`         — rendered right now; scroll to it.
 * - `mode-hidden`     — this session renders it, the window's appearance mode
 *                       is filtering it out by kind.
 * - `off-path`        — not in the transcript at all: the session file keeps
 *                       branches that were rewound away, and the transcript
 *                       renders one path through them.
 */
export type JumpTarget = "visible" | "mode-hidden" | "off-path";

/**
 * Every message uuid a turn can be addressed by. Most carry a `uuids` list; a
 * task notification carries a lone `uuid`; a system or summary line carries
 * none, and nothing ever points at those.
 *
 * The DOM anchor (`data-msg-uuids`) is built from this same function, so what
 * the locator considers reachable and what the page can actually scroll to
 * cannot drift apart.
 */
export function turnUuids(t: RenderedTurn): string[] {
  if ("uuids" in t) return t.uuids;
  return "uuid" in t ? [t.uuid] : [];
}

/** React keys for the rendered turns: the first uuid, so a turn keeps its component (and its open/closed state)
 *  when the list shifts around it. Turns without one fall back to their index. */
export function turnKeys(turns: RenderedTurn[]): string[] {
  const seen = new Set<string>();
  return turns.map((t, i) => {
    let k = turnUuids(t)[0] ?? `#${i}`;
    if (seen.has(k)) k = `${k}#${i}`;
    seen.add(k);
    return k;
  });
}

const covers = (t: RenderedTurn, uuid: string) => turnUuids(t).includes(uuid);

/**
 * Global find searches the whole file while the transcript renders one path
 * through it, so a result can point at something this view will never show —
 * and scrolling to nothing just reads as a broken click.
 *
 * Answered from the turn data rather than the DOM on purpose: a `querySelector`
 * miss cannot tell "not in this session" from "not laid out yet".
 */
export function locateJump(
  uuid: string,
  all: RenderedTurn[],
  visible: RenderedTurn[],
): JumpTarget {
  if (visible.some((t) => covers(t, uuid))) return "visible";
  if (all.some((t) => covers(t, uuid))) return "mode-hidden";
  return "off-path";
}
