/** One rendered line of a Bash tool call: a `#` note, or a line of the script. */
export interface BashLine {
  kind: "note" | "cmd";
  text: string;
}

/** 300000 → "5m", 30000 → "30s" — the raw millisecond count says little at a
 *  glance, and these are always round numbers in practice. */
function humanMs(ms: number): string {
  if (ms % 60000 === 0) return `${ms / 60000}m`;
  if (ms % 1000 === 0) return `${ms / 1000}s`;
  return `${ms}ms`;
}

/**
 * A Bash call as a shell snippet: its description and settings as `#` comments,
 * then the script itself, one line per line.
 *
 * Half of all Bash calls in a working corpus are multi-line, and as JSON every
 * one of those newlines arrives as a literal `\n` inside one long string —
 * which is why a heredoc or a loop reads as a wall of escapes.
 *
 * Returns null when there's no string command, so anything unexpected falls
 * back to the JSON view.
 */
export function bashLines(input: Record<string, unknown>): BashLine[] | null {
  const command = input.command;
  if (typeof command !== "string" || command === "") return null;

  const notes: BashLine[] = [];
  if (typeof input.description === "string" && input.description) {
    notes.push({ kind: "note", text: `# ${input.description}` });
  }
  if (input.run_in_background === true) {
    notes.push({ kind: "note", text: "# runs in the background" });
  }
  if (typeof input.timeout === "number" && input.timeout > 0) {
    notes.push({ kind: "note", text: `# timeout ${humanMs(input.timeout)}` });
  }
  return [...notes, ...command.split("\n").map((text): BashLine => ({ kind: "cmd", text }))];
}

/** The lines as text — what the transcript renders AND what the find bar
 *  matches, so offsets agree. */
export function bashText(lines: BashLine[]): string {
  return lines.map((l) => l.text).join("\n");
}
