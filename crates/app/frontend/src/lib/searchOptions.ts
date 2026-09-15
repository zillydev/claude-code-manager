/** Global find's operators, as the "Search options" legend lists them. */
export type SearchOperator = readonly [op: string, description: string];

export const SEARCH_OPERATORS: readonly SearchOperator[] = [
  ["in:", "match project name"],
  ["from:", "user or assistant messages"],
  ["before:", "messages before a date/time"],
  ["after:", "messages after a date/time"],
  ["tag:", "sessions carrying a tag"],
  ["is:bookmarked", "bookmarked messages only"],
  ["tool:", "turns that called a tool (Bash, Write…)"],
  ["has:link", "messages containing a URL"],
  ["session:", "one session, by name or id"],
];

/** The word being typed: everything after the last space, empty right after one. */
export function trailingToken(input: string): string {
  return input.split(/\s+/).pop() ?? "";
}

/** Operators the word being typed could become — by its key, or by the fixed value of one like
 *  `is:bookmarked`. Nothing once the word has a `:`; its value chips take over from there. */
export function matchOperators(token: string): SearchOperator[] {
  if (token === "") return [...SEARCH_OPERATORS];
  if (token.includes(":")) return [];
  const t = token.toLowerCase();
  return SEARCH_OPERATORS.filter(([op]) => {
    const [key, value] = op.split(":");
    // A value needs two letters: one would pull `has:link` up for any word starting with "l".
    return `${key}:`.startsWith(t) || (value !== "" && t.length >= 2 && value.startsWith(t));
  });
}

/** `input` with the partly typed word replaced by `insert`. */
export function replaceTrailingToken(input: string, insert: string): string {
  return input.slice(0, input.length - trailingToken(input).length) + insert;
}

/** Tool names containing `typed`, in the given (most used first) order, minus one already typed in full. */
export function suggestTools(tools: { name: string; count: number }[], typed: string, limit = 8): string[] {
  const t = typed.toLowerCase();
  return tools
    .map((x) => x.name)
    .filter((n) => n.toLowerCase().includes(t) && n.toLowerCase() !== t)
    .slice(0, limit);
}
