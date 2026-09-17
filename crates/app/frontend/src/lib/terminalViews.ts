export type TerminalStatus = "live" | "exited" | "busy";

/** The terminal views worth keeping mounted: the current session's, and any whose `claude` still runs. The rest
 *  (never started, exited, or owned by another window) hold an xterm for nothing; they come back on return. */
export function keptTerminals<T>(
  opened: Record<string, T>,
  current: string | null,
  status: Record<string, TerminalStatus>,
): Record<string, T> {
  const drop = Object.keys(opened).filter((sid) => sid !== current && status[sid] !== "live");
  if (drop.length === 0) return opened;
  const kept = { ...opened };
  for (const sid of drop) delete kept[sid];
  return kept;
}
