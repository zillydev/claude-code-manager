export type SessionTab = "manager" | "terminal";

/** The tab a session opens to when switching to it from another: the one it was last left on, except that a
 *  Terminal whose claude isn't running falls back to Manager, since showing it would start `claude --resume`. */
export function tabOnOpen(remembered: SessionTab | undefined, current: SessionTab, terminalLive: boolean): SessionTab {
  if (!terminalLive) return "manager";
  return remembered ?? current;
}
