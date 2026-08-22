/** One rendered line of an AskUserQuestion body. */
export interface AskLine {
  kind: "question" | "option" | "blank";
  text: string;
  /** For an option row: its label, so the renderer can mark the chosen one. */
  label?: string;
}

interface RawOption {
  label?: unknown;
  description?: unknown;
}
interface RawQuestion {
  question?: unknown;
  header?: unknown;
  multiSelect?: unknown;
  options?: unknown;
}

const str = (v: unknown): string => (typeof v === "string" ? v : "");

/**
 * AskUserQuestion's input as displayable lines: the question (prefixed with
 * its short header) followed by its options.
 *
 * Returns null when the input isn't the expected shape, so a malformed call
 * falls back to the JSON view rather than rendering an empty card.
 */
export function askLines(input: Record<string, unknown>): AskLine[] | null {
  const questions = input.questions;
  if (!Array.isArray(questions) || questions.length === 0) return null;

  const out: AskLine[] = [];
  for (const [i, raw] of questions.entries()) {
    const q = raw as RawQuestion;
    const text = str(q.question);
    const options = Array.isArray(q.options) ? (q.options as RawOption[]) : [];
    if (!text && options.length === 0) return null;
    if (i > 0) out.push({ kind: "blank", text: "" });

    const header = str(q.header);
    const multi = q.multiSelect === true ? " (pick any)" : "";
    out.push({ kind: "question", text: header ? `${header} · ${text}${multi}` : `${text}${multi}` });
    for (const o of options) {
      const label = str(o.label);
      const desc = str(o.description);
      out.push({ kind: "option", label, text: `  • ${desc ? `${label} — ${desc}` : label}` });
    }
  }
  return out;
}

/** The lines as plain text — what the transcript renders AND what the find bar
 *  matches, so a hit inside a collapsed question paints at the right offset. */
export function askText(lines: AskLine[]): string {
  return lines.map((l) => l.text).join("\n");
}

/**
 * The option labels the user picked, read from the tool result — which reads
 * `Your questions have been answered: "<question>"="<answer>". …`, with
 * multi-select answers comma-joined.
 *
 * Best-effort by design: an unparsed result (an error, or a future wording)
 * simply means nothing is marked, never a wrong option marked.
 */
export function chosenLabels(result: string | null | undefined): Set<string> {
  const out = new Set<string>();
  if (!result) return out;
  for (const m of result.matchAll(/"[^"]*"="([^"]*)"/g)) {
    for (const part of m[1].split(", ")) {
      const label = part.trim();
      if (label) out.add(label);
    }
  }
  return out;
}
