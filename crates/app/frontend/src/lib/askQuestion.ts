/** One rendered line of an AskUserQuestion body. */
export interface AskLine {
  kind: "question" | "option" | "typed" | "preview" | "blank";
  text: string;
  /** For an option row: its label. */
  label?: string;
  /** An option the user picked, or an answer they typed — the renderer accents it. */
  chosen?: boolean;
}

/** One question's answer: the value as written, and its comma-split picks. */
export interface AskAnswer {
  value: string;
  items: string[];
}

interface RawOption {
  label?: unknown;
  description?: unknown;
  /** Shown by Claude Code while the option is highlighted; kept for the pick. */
  preview?: unknown;
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
export function askLines(
  input: Record<string, unknown>,
  result: string | null = null,
): AskLine[] | null {
  const questions = input.questions;
  if (!Array.isArray(questions) || questions.length === 0) return null;

  const answers = parseAnswers(
    result,
    questions.map((raw) => str((raw as RawQuestion).question)),
  );
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
    const answer = answers.get(text);
    const labels = new Set(options.map((o) => str(o.label)));
    // A single pick is written raw, commas and all, so the whole value is a
    // label before it is a list of picks.
    const whole = !!answer && labels.has(answer.value);
    let picked = 0;
    for (const o of options) {
      const label = str(o.label);
      const desc = str(o.description);
      const chosen = !!answer && (answer.value === label || (!whole && answer.items.includes(label)));
      if (chosen) picked++;
      out.push({ kind: "option", label, chosen, text: `  • ${desc ? `${label} — ${desc}` : label}` });
      // The picked option's preview, as Claude Code showed it at decision time.
      const preview = chosen ? str(o.preview) : "";
      for (const line of preview ? preview.split("\n") : []) {
        out.push({ kind: "preview", text: `      ${line}` });
      }
    }
    // Whatever the user typed rather than picked gets its own row, so a card
    // never reads as unanswered. Leftover fragments of a plain value are
    // adjacent pieces of one typed answer, so they re-join.
    if (answer && !whole) {
      const typed =
        picked === 0 ? answer.value : answer.items.filter((it) => !labels.has(it)).join(", ");
      if (typed.trim()) out.push({ kind: "typed", chosen: true, text: `  ↳ ${typed.trim()}` });
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
 * Each question's answer, read from the tool result — `"<question>"="<value>"`
 * pairs, comma-separated, then a closing sentence. Questions are located
 * literally (they may contain quotes themselves), and each value runs to the
 * next question. A single pick is the label written raw, commas included;
 * several picks are comma-joined, with a pick that contains a comma CSV-quoted
 * (`""Small ones: 1, 2", "Other""`); an answer typed with no option picked
 * reads `=(no option selected) notes: <text>`.
 *
 * Best-effort by design: an unparsed result (an error, or a future wording)
 * simply means nothing is marked, never a wrong option marked.
 */
export function parseAnswers(
  result: string | null | undefined,
  questions: string[],
): Map<string, AskAnswer> {
  const out = new Map<string, AskAnswer>();
  if (!result) return out;
  const found = questions
    .map((q) => ({ q, idx: result.indexOf(`"${q}"=`) }))
    .filter((e) => e.idx >= 0)
    .sort((a, b) => a.idx - b.idx);
  found.forEach((e, i) => {
    const start = e.idx + e.q.length + 3;
    const next = found[i + 1];
    // ", " precedes the next pair; the last value runs into the closing sentence.
    const raw = next ? result.slice(start, next.idx - 2) : result.slice(start).replace(CLOSER, "");
    const answer = parseValue(raw);
    if (answer) out.set(e.q, answer);
  });
  return out;
}

const CLOSER = /\. (?:You can now continue|Read the answers carefully)[\s\S]*$/;

function parseValue(raw: string): AskAnswer | null {
  // A pick with a preview: `"<label>" selected preview:\n<text>` — the text is
  // the option's own preview field, quotes and all, so only the label is read.
  const previewAt = raw.startsWith('"') ? raw.indexOf('" selected preview:') : -1;
  if (previewAt > 0) {
    const label = raw.slice(1, previewAt);
    return { value: label, items: [label] };
  }
  // Unknown closing wording: a quoted value still ends at its last quote.
  if (raw.startsWith('"') && !raw.endsWith('"')) raw = raw.slice(0, raw.lastIndexOf('"') + 1);
  if (raw.length >= 4 && raw.startsWith('""') && raw.endsWith('""')) {
    // Quoted items split only at the quote-comma-quote seam, so their commas survive.
    const items = raw.slice(2, -2).split('", "');
    return { value: items.join(", "), items };
  }
  if (raw.length >= 2 && raw.startsWith('"') && raw.endsWith('"')) {
    const plain = raw.slice(1, -1);
    return { value: plain, items: plain.split(", ") };
  }
  const notes = raw.match(/^\(no option selected\)\s*notes:\s*([\s\S]*)$/);
  if (notes) {
    const value = notes[1].trim();
    return value ? { value, items: [value] } : null;
  }
  return null;
}
