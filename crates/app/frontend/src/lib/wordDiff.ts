/** Word-level emphasis inside a diff: which characters of a replaced line actually changed. */
import type { DiffLine } from "./toolDiff";
import type { Token } from "./syntax";

/** [start, end) character offsets into a line's text. */
export type Range = [number, number];

// Runs of whitespace, runs of letters/digits, or single other characters.
const WORD = /\s+|[\p{L}\p{N}_]+|[^\s\p{L}\p{N}_]/gu;
/** Past this the middle of two lines isn't worth an LCS table; it all counts as changed. */
const MAX_CELLS = 250_000;
/** Below this share of common non-space text, emphasis would mark nearly everything. */
const MIN_SHARED = 0.4;

/** Which tokens of each side belong to their longest common subsequence. */
function kept(ta: string[], tb: string[]): [boolean[], boolean[]] {
  const ka = ta.map(() => false);
  const kb = tb.map(() => false);
  let pre = 0;
  while (pre < ta.length && pre < tb.length && ta[pre] === tb[pre]) {
    ka[pre] = kb[pre] = true;
    pre++;
  }
  let suf = 0;
  while (suf < ta.length - pre && suf < tb.length - pre && ta[ta.length - 1 - suf] === tb[tb.length - 1 - suf]) {
    ka[ta.length - 1 - suf] = kb[tb.length - 1 - suf] = true;
    suf++;
  }
  const n = ta.length - pre - suf;
  const m = tb.length - pre - suf;
  if (n === 0 || m === 0 || n * m > MAX_CELLS) return [ka, kb];

  // lcs[i * (m + 1) + j] = LCS length of the middles from i and j on.
  const lcs = new Uint32Array((n + 1) * (m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i * (m + 1) + j] =
        ta[pre + i] === tb[pre + j]
          ? lcs[(i + 1) * (m + 1) + j + 1] + 1
          : Math.max(lcs[(i + 1) * (m + 1) + j], lcs[i * (m + 1) + j + 1]);
    }
  }
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (ta[pre + i] === tb[pre + j]) {
      ka[pre + i] = kb[pre + j] = true;
      i++;
      j++;
    } else if (lcs[(i + 1) * (m + 1) + j] >= lcs[i * (m + 1) + j + 1]) {
      i++;
    } else {
      j++;
    }
  }
  return [ka, kb];
}

/** Offsets of the tokens not kept, with neighbours joined across whitespace-only gaps. */
function rangesOf(text: string, tokens: string[], keep: boolean[]): Range[] {
  const out: Range[] = [];
  let pos = 0;
  tokens.forEach((t, i) => {
    const end = pos + t.length;
    if (!keep[i]) {
      const last = out[out.length - 1];
      if (last && /^\s*$/.test(text.slice(last[1], pos))) last[1] = end;
      else out.push([pos, end]);
    }
    pos = end;
  });
  return out;
}

const nonSpace = (tokens: string[], keep?: boolean[]) =>
  tokens.reduce((n, t, i) => (keep && !keep[i]) || /^\s+$/.test(t) ? n : n + t.length, 0);

export function changedRanges(a: string, b: string): { a: Range[]; b: Range[] } | null {
  const ta = a.match(WORD) ?? [];
  const tb = b.match(WORD) ?? [];
  const [ka, kb] = kept(ta, tb);
  // A pure insertion or deletion inside a line is always worth pointing at, however long.
  const oneSideWhole = ka.every(Boolean) || kb.every(Boolean);
  const shared = nonSpace(ta, ka) / Math.max(nonSpace(ta), nonSpace(tb), 1);
  if (!oneSideWhole && shared < MIN_SHARED) return null;
  return { a: rangesOf(a, ta, ka), b: rangesOf(b, tb, kb) };
}

export function wordEmphasis(lines: DiffLine[]): (Range[] | undefined)[] {
  const out: (Range[] | undefined)[] = lines.map(() => undefined);
  let i = 0;
  while (i < lines.length) {
    if (lines[i].kind === "ctx") {
      i++;
      continue;
    }
    const dels: number[] = [];
    const adds: number[] = [];
    for (; i < lines.length && lines[i].kind !== "ctx"; i++) {
      (lines[i].kind === "del" ? dels : adds).push(i);
    }
    for (let k = 0; k < Math.min(dels.length, adds.length); k++) {
      const r = changedRanges(lines[dels[k]].text, lines[adds[k]].text);
      if (r) {
        out[dels[k]] = r.a;
        out[adds[k]] = r.b;
      }
    }
  }
  return out;
}

export function emphasize(tokens: Token[], ranges: Range[]): (Token & { emph: boolean })[] {
  const out: (Token & { emph: boolean })[] = [];
  let pos = 0;
  let r = 0;
  for (const t of tokens) {
    let off = 0;
    while (off < t.text.length) {
      const at = pos + off;
      while (r < ranges.length && ranges[r][1] <= at) r++;
      const inside = r < ranges.length && ranges[r][0] <= at;
      const edge = inside ? ranges[r][1] : r < ranges.length ? ranges[r][0] : Infinity;
      const take = Math.min(t.text.length - off, edge - at);
      out.push({ ...t, text: t.text.slice(off, off + take), emph: inside });
      off += take;
    }
    pos += t.text.length;
  }
  return out;
}
