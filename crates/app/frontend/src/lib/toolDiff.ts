/** One rendered line of a tool's body. `ctx` is unchanged context.
 *
 *  `side`/`index` say where the line came from, so the renderer can borrow the
 *  syntax tokens of that side — highlighting each side as a whole keeps
 *  multi-line strings and comments intact, which per-line highlighting can't. */
export interface DiffLine {
  kind: "ctx" | "add" | "del";
  text: string;
  side?: "old" | "new";
  index?: number;
}

/** Past this many lines the O(n·m) table isn't worth it — such an "edit" is a
 *  wholesale replacement anyway, so show it as one deletion and one addition. */
const LCS_LINE_LIMIT = 1500;

/** Line-level diff via LCS. Pure, so the renderer and the find matcher can
 *  derive the SAME text from it (see `diffText`). */
export function diffLines(oldText: string, newText: string): DiffLine[] {
  const a = oldText.split("\n");
  const b = newText.split("\n");

  if (a.length * b.length > LCS_LINE_LIMIT * LCS_LINE_LIMIT) {
    return [
      ...a.map((text, index): DiffLine => ({ kind: "del", text, side: "old", index })),
      ...b.map((text, index): DiffLine => ({ kind: "add", text, side: "new", index })),
    ];
  }

  // lcs[i][j] = length of the longest common subsequence of a[i..] and b[j..].
  const lcs: number[][] = Array.from({ length: a.length + 1 }, () =>
    new Array<number>(b.length + 1).fill(0),
  );
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }

  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      out.push({ kind: "ctx", text: a[i], side: "new", index: j });
      i++;
      j++;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      out.push({ kind: "del", text: a[i], side: "old", index: i });
      i++;
    } else {
      out.push({ kind: "add", text: b[j], side: "new", index: j });
      j++;
    }
  }
  while (i < a.length) {
    out.push({ kind: "del", text: a[i], side: "old", index: i });
    i++;
  }
  while (j < b.length) {
    out.push({ kind: "add", text: b[j], side: "new", index: j });
    j++;
  }
  return out;
}

const str = (v: unknown): string | null => (typeof v === "string" ? v : null);

/**
 * The diff view for a tool call, or `null` when the tool isn't one of the
 * file-editing kinds (or its input isn't the shape we expect — a malformed
 * call must fall back to the JSON view rather than render nothing).
 *
 * Claude Code shows these as +/- diffs in the terminal; the transcript used to
 * show pretty-printed JSON with `\n`-escaped strings, which is unreadable for
 * exactly the tools used most (1000+ Edits in a working corpus).
 */
export function toolDiff(name: string, input: Record<string, unknown>): DiffLine[] | null {
  switch (name) {
    case "Edit":
    case "Update": {
      const oldS = str(input.old_string);
      const newS = str(input.new_string);
      return oldS === null || newS === null ? null : diffLines(oldS, newS);
    }
    case "Write": {
      const content = str(input.content);
      // A new file is all additions — there is no "before" to diff against.
      return content === null
        ? null
        : content.split("\n").map((text, index): DiffLine => ({ kind: "add", text, side: "new", index }));
    }
    case "NotebookEdit": {
      const oldS = str(input.old_source);
      const newS = str(input.new_source);
      if (newS === null) return null;
      return diffLines(oldS ?? "", newS);
    }
    case "MultiEdit": {
      const edits = input.edits;
      if (!Array.isArray(edits) || edits.length === 0) return null;
      const out: DiffLine[] = [];
      for (const [n, e] of edits.entries()) {
        const oldS = str((e as Record<string, unknown>)?.old_string);
        const newS = str((e as Record<string, unknown>)?.new_string);
        if (oldS === null || newS === null) return null; // malformed → JSON view
        if (n > 0) out.push({ kind: "ctx", text: "" }); // blank line between hunks
        out.push(...diffLines(oldS, newS));
      }
      return out;
    }
    default:
      return null;
  }
}

/** One line as text, `git diff`-style. The renderer emits exactly this per
 *  line (plus a newline), so a line's prefix can never drift between what is
 *  displayed and what is searched. */
export function diffLineText(line: DiffLine): string {
  const mark = line.kind === "add" ? "+" : line.kind === "del" ? "-" : " ";
  return `${mark} ${line.text}`;
}

/** The whole diff as text. This is what the transcript renders AND what the
 *  find bar matches against — one derivation, so a match inside a collapsed
 *  edit paints at the right offsets once it expands. */
export function diffText(lines: DiffLine[]): string {
  return lines.map(diffLineText).join("\n");
}

/** The two texts a tool's diff was computed from, so the renderer can
 *  highlight each side once and index into the result. `null` for tools with
 *  no diff view, and for MultiEdit — its hunks come from many texts, so its
 *  lines carry no usable `index` and render unhighlighted. */
export function diffSources(
  name: string,
  input: Record<string, unknown>,
): { old: string; new: string; path: string | null } | null {
  const s = (v: unknown) => (typeof v === "string" ? v : null);
  const path = s(input.file_path) ?? s(input.notebook_path);
  switch (name) {
    case "Edit":
    case "Update": {
      const o = s(input.old_string);
      const n = s(input.new_string);
      return o === null || n === null ? null : { old: o, new: n, path };
    }
    case "Write": {
      const c = s(input.content);
      return c === null ? null : { old: "", new: c, path };
    }
    case "NotebookEdit": {
      const n = s(input.new_source);
      return n === null ? null : { old: s(input.old_source) ?? "", new: n, path };
    }
    default:
      return null;
  }
}
