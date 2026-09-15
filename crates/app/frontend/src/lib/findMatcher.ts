/** Query → span finder for the in-session find bar. Pure; DOM-free. */

export interface FindOpts {
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
}

export interface Span {
  start: number;
  end: number;
}

export type Matcher =
  | { ok: true; find: (text: string) => Span[] }
  | { ok: false; error: string };

export function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Compile the bar's query + toggles into a reusable `find(text)`. Literal
 *  mode escapes metacharacters; whole-word wraps in `\b…\b`; the flags are
 *  always `g` (+`i` unless case-sensitive). Zero-width matches are skipped so
 *  `a*` can't loop forever. An invalid regex reports the engine's message. */
export function compileMatcher(query: string, opts: FindOpts): Matcher {
  if (query === "") return { ok: true, find: () => [] };

  let pattern = opts.regex ? query : escapeRegExp(query);
  if (opts.wholeWord) pattern = `\\b(?:${pattern})\\b`;

  let re: RegExp;
  try {
    re = new RegExp(pattern, opts.caseSensitive ? "g" : "gi");
  } catch (e) {
    return { ok: false, error: (e as Error).message };
  }

  return {
    ok: true,
    find: (text: string) => {
      const out: Span[] = [];
      re.lastIndex = 0;
      let m: RegExpExecArray | null;
      while ((m = re.exec(text)) !== null) {
        if (m[0].length === 0) {
          re.lastIndex = m.index + 1; // zero-width guard
        } else {
          out.push({ start: m.index, end: m.index + m[0].length });
        }
        if (out.length > 100_000) break; // runaway cap
      }
      return out;
    },
  };
}
