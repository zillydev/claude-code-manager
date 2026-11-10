/** Terminal output as styled text runs: SGR styling kept, every other escape dropped (no cursor
 *  emulation). One derivation for the renderer (AnsiText) and find, so painted offsets match. */

export interface AnsiStyle {
  fg?: string;
  bg?: string;
  bold?: true;
  dim?: true;
  italic?: true;
  underline?: true;
  inverse?: true;
  strike?: true;
}

export interface AnsiSeg {
  text: string;
  style?: AnsiStyle;
}

const ESC = "\x1b";
const CUBE = [0, 95, 135, 175, 215, 255];

const hex2 = (n: number) => n.toString(16).padStart(2, "0");

/** A 256-colour index: the 16 theme slots stay CSS variables so a theme switch recolours them. */
function color256(n: number): string | undefined {
  if (!Number.isInteger(n) || n < 0 || n > 255) return undefined;
  if (n < 16) return `var(--ansi-${n})`;
  if (n >= 232) {
    const g = hex2(8 + 10 * (n - 232));
    return `#${g}${g}${g}`;
  }
  const i = n - 16;
  return `#${hex2(CUBE[Math.floor(i / 36)])}${hex2(CUBE[Math.floor(i / 6) % 6])}${hex2(CUBE[i % 6])}`;
}

function applySgr(style: AnsiStyle, params: string): AnsiStyle {
  const s: AnsiStyle = { ...style };
  const ps = params === "" ? [0] : params.split(";").map((p) => (p === "" ? 0 : Number(p)));
  for (let i = 0; i < ps.length; i++) {
    const p = ps[i];
    if (p === 0) {
      for (const k of Object.keys(s)) delete s[k as keyof AnsiStyle];
    } else if (p === 1) s.bold = true;
    else if (p === 2) s.dim = true;
    else if (p === 3) s.italic = true;
    else if (p === 4) s.underline = true;
    else if (p === 7) s.inverse = true;
    else if (p === 9) s.strike = true;
    else if (p === 22) {
      delete s.bold;
      delete s.dim;
    } else if (p === 23) delete s.italic;
    else if (p === 24) delete s.underline;
    else if (p === 27) delete s.inverse;
    else if (p === 29) delete s.strike;
    else if (p >= 30 && p <= 37) s.fg = `var(--ansi-${p - 30})`;
    else if (p >= 90 && p <= 97) s.fg = `var(--ansi-${p - 82})`;
    else if (p === 39) delete s.fg;
    else if (p >= 40 && p <= 47) s.bg = `var(--ansi-${p - 40})`;
    else if (p >= 100 && p <= 107) s.bg = `var(--ansi-${p - 92})`;
    else if (p === 49) delete s.bg;
    else if (p === 38 || p === 48) {
      let c: string | undefined;
      if (ps[i + 1] === 5) {
        c = color256(ps[i + 2]);
        i += 2;
      } else if (ps[i + 1] === 2) {
        const [r, g, b] = ps.slice(i + 2, i + 5);
        if ([r, g, b].every((v) => Number.isInteger(v) && v >= 0 && v <= 255)) c = `rgb(${r},${g},${b})`;
        i += 4;
      }
      if (c) s[p === 38 ? "fg" : "bg"] = c;
    }
  }
  return s;
}

function sameStyle(a?: AnsiStyle, b?: AnsiStyle): boolean {
  const ka = a ? Object.keys(a) : [];
  const kb = b ? Object.keys(b) : [];
  return ka.length === kb.length && ka.every((k) => a![k as keyof AnsiStyle] === b?.[k as keyof AnsiStyle]);
}

/** Index just past the escape sequence starting at `raw[i]` (an ESC), plus SGR params when it is one. */
function scanEscape(raw: string, i: number): { end: number; sgr?: string } {
  const n = raw.length;
  const next = raw[i + 1];
  if (next === undefined) return { end: n };
  if (next === "[") {
    let j = i + 2;
    while (j < n && raw.charCodeAt(j) >= 0x20 && raw.charCodeAt(j) <= 0x3f) j++;
    if (j >= n) return { end: n };
    const c = raw.charCodeAt(j);
    if (c < 0x40 || c > 0x7e) return { end: j };
    const params = raw.slice(i + 2, j);
    // A private-parameter prefix (`<=>?`) makes it a mode set, not a style.
    const isSgr = raw[j] === "m" && !/^[<=>?]/.test(params);
    return { end: j + 1, sgr: isSgr ? params : undefined };
  }
  if (next === "]") {
    for (let j = i + 2; j < n; j++) {
      if (raw[j] === "\x07") return { end: j + 1 };
      if (raw[j] === ESC && raw[j + 1] === "\\") return { end: j + 2 };
    }
    return { end: n };
  }
  const c = raw.charCodeAt(i + 1);
  if (c >= 0x20 && c <= 0x2f) {
    let j = i + 1;
    while (j < n && raw.charCodeAt(j) >= 0x20 && raw.charCodeAt(j) <= 0x2f) j++;
    return { end: j < n && raw.charCodeAt(j) < 0x80 ? j + 1 : j };
  }
  if (c >= 0x30 && c <= 0x7e) return { end: i + 2 };
  return { end: i + 1 };
}

/** Styled runs of `raw`. A `\r` erases its line only once more text follows, and then the whole
 *  line: "last redraw wins" is a deliberate simplification of a terminal's overwrite. */
export function ansiSegments(raw: string): AnsiSeg[] {
  if (!raw.includes(ESC) && !raw.includes("\r")) return [{ text: raw }];

  const out: AnsiSeg[] = [];
  let total = 0;
  let lineStart = 0;
  let pendingCr = false;
  let style: AnsiStyle = {};

  const truncate = (len: number) => {
    while (total > len) {
      const last = out[out.length - 1];
      const drop = Math.min(last.text.length, total - len);
      last.text = last.text.slice(0, last.text.length - drop);
      total -= drop;
      if (!last.text) out.pop();
    }
  };
  const emit = (text: string) => {
    if (!text) return;
    const st = Object.keys(style).length ? style : undefined;
    const last = out[out.length - 1];
    if (last && sameStyle(last.style, st)) last.text += text;
    else out.push(st ? { text, style: st } : { text });
    total += text.length;
  };
  const emitPlain = (chunk: string) => {
    let start = 0;
    for (let k = 0; k < chunk.length; k++) {
      const ch = chunk[k];
      if (ch !== "\r" && ch !== "\n") {
        if (pendingCr) {
          truncate(lineStart);
          pendingCr = false;
        }
        continue;
      }
      emit(chunk.slice(start, k));
      start = k + 1;
      if (ch === "\r") pendingCr = true;
      else {
        pendingCr = false;
        emit("\n");
        lineStart = total;
      }
    }
    emit(chunk.slice(start));
  };

  let i = 0;
  while (i < raw.length) {
    const e = raw.indexOf(ESC, i);
    if (e === -1) {
      emitPlain(raw.slice(i));
      break;
    }
    emitPlain(raw.slice(i, e));
    const { end, sgr } = scanEscape(raw, e);
    if (sgr !== undefined) style = applySgr(style, sgr);
    i = end;
  }
  return out.length ? out : [{ text: "" }];
}

/** The plain text of `raw` as rendered: ansiSegments' runs joined. */
export function ansiText(raw: string): string {
  return ansiSegments(raw)
    .map((s) => s.text)
    .join("");
}
