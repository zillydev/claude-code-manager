/** Discord-style query: `key:value` operators (values may be quoted) + free text. */
export interface SearchQuery {
  text: string;
  project?: string;
  role?: "user" | "assistant";
  /** Epoch seconds, local midnight. `after` inclusive, `before` exclusive. */
  after?: number;
  before?: number;
  tags: string[];
  bookmarked: boolean;
  tool?: string;
  hasLink: boolean;
  session?: string;
  /** Human-readable problems (e.g. a malformed date) — shown inline, never fatal. */
  errors: string[];
}

const KEYS = new Set(["in", "from", "before", "after", "tag", "is", "tool", "has", "session"]);

function localMidnight(d: string): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(d);
  if (!m) return null;
  const t = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  return Number.isNaN(t.getTime()) ? null : Math.floor(t.getTime() / 1000);
}

/** Parse; unknown `foo:bar` stays literal text (Discord behaviour). */
export function parseSearchQuery(input: string): SearchQuery {
  const q: SearchQuery = { text: "", tags: [], bookmarked: false, hasLink: false, errors: [] };
  const free: string[] = [];
  const re = /(\w+):(?:"([^"]*)"|(\S+))|"([^"]*)"|(\S+)/g;
  for (const m of input.matchAll(re)) {
    const [, key, qv, bv, quoted, bare] = m;
    if (key !== undefined) {
      const val = qv ?? bv ?? "";
      const k = key.toLowerCase();
      if (!KEYS.has(k)) {
        free.push(m[0]);
        continue;
      }
      switch (k) {
        case "in": q.project = val; break;
        case "session": q.session = val; break;
        case "tool": q.tool = val; break;
        case "tag": if (val) q.tags.push(val); break;
        case "from": {
          const v = val.toLowerCase();
          if (v === "user" || v === "assistant") q.role = v;
          else q.errors.push(`from: expects user|assistant, got "${val}"`);
          break;
        }
        case "is":
          if (val.toLowerCase() === "bookmarked") q.bookmarked = true;
          else q.errors.push(`is: expects bookmarked, got "${val}"`);
          break;
        case "has":
          if (val.toLowerCase() === "link") q.hasLink = true;
          else q.errors.push(`has: expects link, got "${val}"`);
          break;
        case "before":
        case "after": {
          const ts = localMidnight(val);
          if (ts === null) q.errors.push(`${k}: expects yyyy-mm-dd, got "${val}"`);
          else if (k === "after") q.after = ts;
          else q.before = ts;
          break;
        }
      }
    } else {
      free.push(quoted ?? bare ?? "");
    }
  }
  q.text = free.join(" ").trim();
  return q;
}

/** Whether the query narrows anything at all — an empty query must not trigger
 *  a whole-disk walk. */
export function isRunnable(q: SearchQuery): boolean {
  return (
    q.text.length > 0 || q.bookmarked || q.hasLink ||
    !!(q.project || q.role || q.tool || q.session || q.after || q.before || q.tags.length)
  );
}
