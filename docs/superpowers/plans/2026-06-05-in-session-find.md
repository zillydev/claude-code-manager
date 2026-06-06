# In-session Find Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a text-editor-style find bar to the open transcript in the webui — live matching, case/word/regex toggles, highlight-all + active match, next/prev with wrap — read-only and entirely frontend.

**Architecture:** Matches are computed from two exact-by-construction sources: "exact" segments (user/system/summary text, tool name/input/result) matched off the data-model string, and markdown segments (assistant text) matched off the live rendered `textContent`. Every searchable DOM element carries a stable `data-seg-id`. Painting uses the CSS Custom Highlight API (`::highlight()` over `Range` objects), feature-detected. Compact mode is suspended while find is open so counts are exact; a collapsed tool call auto-expands when you navigate to a match inside it. Paint + scroll run in a `useLayoutEffect` after the reveal/expand re-render commits.

**Tech Stack:** React 19 + TypeScript, Tailwind v4, react-markdown@10, vitest (new — pure-matcher unit tests), CSS Custom Highlight API.

---

## File Structure

New:
- `crates/webui/frontend/src/lib/search.ts` — pure matcher (`compileMatcher`); types `FindOpts`, `Span`, `Matcher`.
- `crates/webui/frontend/src/lib/search.test.ts` — vitest unit tests for the matcher.
- `crates/webui/frontend/src/lib/segments.ts` — seg-id helpers, document-order ranking, `exactMatches`, `compareMatches`; types `SegField`, `FindMatch`.
- `crates/webui/frontend/src/lib/segments.test.ts` — vitest unit tests for seg-id + `exactMatches`.
- `crates/webui/frontend/src/lib/highlight.ts` — DOM offset→`Range` mapping, markdown DOM matches, CSS-Highlight painting, feature-detect, clear. (Manual-verified — not unit-tested.)
- `crates/webui/frontend/src/hooks/useFind.ts` — find state, match computation orchestration, navigation, reveal, paint effects.
- `crates/webui/frontend/src/components/FindBar.tsx` — the find bar UI.

Modified:
- `crates/webui/frontend/package.json` — add `vitest` devDep + `test` script.
- `crates/webui/frontend/src/components/ToolCall.tsx` — `forceExpand` prop + `data-seg-id` on name/input/result; takes `turnIndex`/`blockIndex`.
- `crates/webui/frontend/src/components/Transcript.tsx` — wire `useFind`, render `FindBar`, suspend compact, keyboard, container ref, thread `turnIndex`/`blockIndex`, `data-seg-id` on text segments.
- `crates/webui/frontend/src/index.css` — `::highlight()` rules + flash-fallback class.

---

## Task 1: vitest tooling + the full matcher test suite (red)

**Files:**
- Modify: `crates/webui/frontend/package.json`
- Create: `crates/webui/frontend/src/lib/search.test.ts`

- [ ] **Step 1: Add vitest devDependency + test script**

Edit `crates/webui/frontend/package.json`. Add to `"scripts"`:

```json
    "test": "vitest run"
```

Add to `"devDependencies"`:

```json
    "vitest": "^3.2.4"
```

- [ ] **Step 2: Install**

Run: `npm --prefix crates/webui/frontend install`
Expected: installs vitest, no errors.

- [ ] **Step 3: Write the full matcher test suite**

Create `crates/webui/frontend/src/lib/search.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { compileMatcher } from "./search";

function spans(query: string, opts: Partial<{ caseSensitive: boolean; wholeWord: boolean; regex: boolean }>, text: string) {
  const m = compileMatcher(query, { caseSensitive: false, wholeWord: false, regex: false, ...opts });
  if (!m.ok) throw new Error("expected ok matcher");
  return m.find(text);
}

describe("compileMatcher", () => {
  it("plain match is case-insensitive by default", () => {
    expect(spans("foo", {}, "FOO foo Foo")).toHaveLength(3);
  });

  it("respects case-sensitive option", () => {
    const s = spans("foo", { caseSensitive: true }, "FOO foo Foo");
    expect(s).toEqual([{ start: 4, end: 7 }]);
  });

  it("escapes regex metacharacters in plain mode", () => {
    const s = spans("a.b", {}, "axb a.b");
    expect(s).toEqual([{ start: 4, end: 7 }]);
  });

  it("whole-word does not match inside larger words", () => {
    const s = spans("cat", { wholeWord: true }, "cat category a cat.");
    expect(s).toHaveLength(2);
    expect(s[0]).toEqual({ start: 0, end: 3 });
  });

  it("regex mode matches a pattern", () => {
    expect(spans("f.o", { regex: true }, "foo fao fxo")).toHaveLength(3);
  });

  it("invalid regex returns an error", () => {
    const m = compileMatcher("(", { caseSensitive: false, wholeWord: false, regex: true });
    expect(m.ok).toBe(false);
  });

  it("zero-width regex terminates and skips empty matches", () => {
    const s = spans("a*", { regex: true }, "aaa bbb");
    expect(s).toEqual([{ start: 0, end: 3 }]);
  });

  it("empty query yields no matches", () => {
    expect(spans("", {}, "anything")).toEqual([]);
  });

  it("returns matches in ascending order", () => {
    const s = spans("x", {}, "x_x_x");
    expect(s.map((m) => m.start)).toEqual([0, 2, 4]);
  });
});
```

- [ ] **Step 4: Run the suite to verify it fails (no implementation yet)**

Run: `npm --prefix crates/webui/frontend run test`
Expected: FAIL — cannot resolve `./search` (module not created until Task 2).

- [ ] **Step 5: Commit**

```bash
git add crates/webui/frontend/package.json crates/webui/frontend/package-lock.json crates/webui/frontend/src/lib/search.test.ts
git commit -m "test(webui): vitest setup + in-session find matcher tests (red)"
```

---

## Task 2: implement the pure matcher (green)

**Files:**
- Create: `crates/webui/frontend/src/lib/search.ts`
- Test: `crates/webui/frontend/src/lib/search.test.ts` (from Task 1)

- [ ] **Step 1: Implement `compileMatcher`**

Create `crates/webui/frontend/src/lib/search.ts`:

```ts
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

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

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
          re.lastIndex = m.index + 1; // zero-width guard against infinite loop
        } else {
          out.push({ start: m.index, end: m.index + m[0].length });
        }
        if (out.length > 100_000) break; // runaway safety cap
      }
      return out;
    },
  };
}
```

- [ ] **Step 2: Run the suite to verify it passes**

Run: `npm --prefix crates/webui/frontend run test`
Expected: PASS — all 9 tests green.

- [ ] **Step 3: Commit**

```bash
git add crates/webui/frontend/src/lib/search.ts
git commit -m "feat(webui): pure find matcher (plain/case/word/regex, zero-width guard)"
```

---

## Task 3: segment model + exact matches (green)

**Files:**
- Create: `crates/webui/frontend/src/lib/segments.ts`
- Create: `crates/webui/frontend/src/lib/segments.test.ts`

- [ ] **Step 1: Write the tests**

Create `crates/webui/frontend/src/lib/segments.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { makeSegId, parseSegId, segRank, compareMatches, exactMatches } from "./segments";
import { compileMatcher } from "./search";
import type { RenderedTurn } from "../types";

const find = (q: string) => {
  const m = compileMatcher(q, { caseSensitive: false, wholeWord: false, regex: false });
  if (!m.ok) throw new Error("bad matcher");
  return m.find;
};

describe("seg-id", () => {
  it("round-trips with a null block index", () => {
    expect(parseSegId(makeSegId(3, null, "user-text"))).toEqual({ turnIndex: 3, blockIndex: null, field: "user-text" });
  });
  it("round-trips with a block index", () => {
    expect(parseSegId(makeSegId(5, 2, "tool-input"))).toEqual({ turnIndex: 5, blockIndex: 2, field: "tool-input" });
  });
});

describe("segRank ordering", () => {
  it("orders by turn, then block, then field", () => {
    const a = segRank(1, 0, "tool-name");
    const b = segRank(1, 0, "tool-input");
    const c = segRank(1, 1, "tool-name");
    const d = segRank(2, 0, "user-text");
    expect(a).toBeLessThan(b);
    expect(b).toBeLessThan(c);
    expect(c).toBeLessThan(d);
  });
});

describe("exactMatches", () => {
  const turns: RenderedTurn[] = [
    { type: "user", timestamp: null, text: "find the cat", uuid: "u1", edit_index: 0, fork_index: 0 },
    {
      type: "assistant",
      timestamp: null,
      model: null,
      uuid: "a1",
      edit_index: 1,
      fork_index: 1,
      blocks: [
        { type: "text", text: "a cat appears" }, // markdown — NOT exact-matched
        {
          type: "tool_call",
          id: "t1",
          name: "Cat",
          input_summary: "",
          input_full: { animal: "cat" },
          result: { text: "one cat found", is_error: false },
        },
      ],
    },
  ];

  it("matches user text, tool name, tool input, tool result — but not markdown text", () => {
    const got = exactMatches(turns, find("cat"));
    const fields = got.map((m) => m.field).sort();
    expect(fields).toEqual(["tool-input", "tool-name", "tool-result", "user-text"]);
  });

  it("sorts into document order with compareMatches", () => {
    const got = exactMatches(turns, find("cat")).sort(compareMatches);
    expect(got[0].field).toBe("user-text");
    expect(got[got.length - 1].field).toBe("tool-result");
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `npm --prefix crates/webui/frontend run test`
Expected: FAIL — cannot resolve `./segments`.

- [ ] **Step 3: Implement `segments.ts`**

Create `crates/webui/frontend/src/lib/segments.ts`:

```ts
import type { RenderedTurn } from "../types";
import type { Span } from "./search";

export type SegField =
  | "user-text"
  | "assistant-text"
  | "system"
  | "summary"
  | "tool-name"
  | "tool-input"
  | "tool-result";

export interface FindMatch {
  segId: string;
  turnIndex: number;
  blockIndex: number | null;
  field: SegField;
  rank: number;
  start: number;
  end: number;
}

export function makeSegId(turnIndex: number, blockIndex: number | null, field: SegField): string {
  return `${turnIndex}:${blockIndex ?? "-"}:${field}`;
}

export function parseSegId(segId: string): { turnIndex: number; blockIndex: number | null; field: SegField } {
  const parts = segId.split(":");
  return {
    turnIndex: Number(parts[0]),
    blockIndex: parts[1] === "-" ? null : Number(parts[1]),
    field: parts[2] as SegField,
  };
}

export function segRank(turnIndex: number, blockIndex: number | null, field: SegField): number {
  const b = blockIndex ?? 0;
  const f = field === "tool-input" ? 1 : field === "tool-result" ? 2 : 0;
  return turnIndex * 1_000_000 + b * 10 + f;
}

export function compareMatches(a: FindMatch, b: FindMatch): number {
  return a.rank - b.rank || a.start - b.start;
}

export function exactMatches(turns: RenderedTurn[], find: (t: string) => Span[]): FindMatch[] {
  const out: FindMatch[] = [];
  const push = (ti: number, bi: number | null, field: SegField, text: string) => {
    for (const s of find(text)) {
      out.push({
        segId: makeSegId(ti, bi, field),
        turnIndex: ti,
        blockIndex: bi,
        field,
        rank: segRank(ti, bi, field),
        start: s.start,
        end: s.end,
      });
    }
  };

  turns.forEach((turn, ti) => {
    switch (turn.type) {
      case "user":
        push(ti, null, "user-text", turn.text);
        break;
      case "system":
        push(ti, null, "system", turn.content);
        break;
      case "summary":
        push(ti, null, "summary", turn.content);
        break;
      case "assistant":
        turn.blocks.forEach((b, bi) => {
          if (b.type === "tool_call") {
            push(ti, bi, "tool-name", b.name);
            push(ti, bi, "tool-input", JSON.stringify(b.input_full, null, 2));
            if (b.result) push(ti, bi, "tool-result", b.result.text);
          }
          // assistant-text is markdown — matched off the DOM, not here.
        });
        break;
    }
  });
  return out;
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `npm --prefix crates/webui/frontend run test`
Expected: PASS — all search + segments tests green.

- [ ] **Step 5: Commit**

```bash
git add crates/webui/frontend/src/lib/segments.ts crates/webui/frontend/src/lib/segments.test.ts
git commit -m "feat(webui): find segment model + exact-source matches"
```

---

## Task 4: DOM highlight module (manual-verified)

**Files:**
- Create: `crates/webui/frontend/src/lib/highlight.ts`

No unit tests: this module needs the CSS Custom Highlight API and real layout (`scrollIntoView`), neither of which jsdom implements. It is verified manually in Task 10. Uses `any`-casts for the Highlight API so it compiles regardless of the TS DOM-lib version.

- [ ] **Step 1: Implement `highlight.ts`**

Create `crates/webui/frontend/src/lib/highlight.ts`:

```ts
import { parseSegId, segRank, type FindMatch } from "./segments";
import type { Span } from "./search";

export function isHighlightSupported(): boolean {
  return typeof CSS !== "undefined" && !!(CSS as any).highlights && typeof (window as any).Highlight === "function";
}

/** Map [start, end) char offsets within el's concatenated text to a DOM Range. */
export function rangeForOffsets(el: Element, start: number, end: number): Range | null {
  const doc = el.ownerDocument;
  const walker = doc.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  let pos = 0;
  let startNode: Text | null = null;
  let startOff = 0;
  let endNode: Text | null = null;
  let endOff = 0;
  let node: Node | null;
  while ((node = walker.nextNode())) {
    const t = node as Text;
    const len = t.data.length;
    if (startNode === null && start <= pos + len) {
      startNode = t;
      startOff = start - pos;
    }
    if (startNode !== null && end <= pos + len) {
      endNode = t;
      endOff = end - pos;
      break;
    }
    pos += len;
  }
  if (!startNode || !endNode) return null;
  const r = doc.createRange();
  r.setStart(startNode, startOff);
  r.setEnd(endNode, endOff);
  return r;
}

/** Matches inside assistant markdown segments, read from the live DOM. */
export function domMarkdownMatches(container: HTMLElement, find: (t: string) => Span[]): FindMatch[] {
  const out: FindMatch[] = [];
  container.querySelectorAll<HTMLElement>("[data-seg-id]").forEach((el) => {
    const segId = el.dataset.segId!;
    const p = parseSegId(segId);
    if (p.field !== "assistant-text") return;
    const text = el.textContent ?? "";
    for (const s of find(text)) {
      out.push({
        segId,
        turnIndex: p.turnIndex,
        blockIndex: p.blockIndex,
        field: "assistant-text",
        rank: segRank(p.turnIndex, p.blockIndex, "assistant-text"),
        start: s.start,
        end: s.end,
      });
    }
  });
  return out;
}

/**
 * Paint cm-find (all mounted, non-active matches) + cm-find-active (the active
 * match) using the CSS Custom Highlight API. Returns the active Range for scroll.
 * Matches whose segId element is not mounted (collapsed tool calls) are skipped.
 */
export function paintHighlights(container: HTMLElement, matches: FindMatch[], activeIndex: number): Range | null {
  const byEl = new Map<string, Element>();
  container.querySelectorAll<HTMLElement>("[data-seg-id]").forEach((el) => byEl.set(el.dataset.segId!, el));

  let activeRange: Range | null = null;

  if (isHighlightSupported()) {
    const HighlightCtor = (window as any).Highlight;
    const rest = new HighlightCtor();
    matches.forEach((m, i) => {
      const el = byEl.get(m.segId);
      if (!el) return;
      const r = rangeForOffsets(el, m.start, m.end);
      if (!r) return;
      if (i === activeIndex) activeRange = r;
      else rest.add(r);
    });
    (CSS as any).highlights.set("cm-find", rest);
    if (activeRange) {
      const act = new HighlightCtor(activeRange);
      act.priority = 1;
      (CSS as any).highlights.set("cm-find-active", act);
    } else {
      (CSS as any).highlights.delete("cm-find-active");
    }
  } else {
    // Fallback: no painting; flash the active segment so it's locatable.
    const m = matches[activeIndex];
    const el = m ? byEl.get(m.segId) : undefined;
    if (el && m) {
      activeRange = rangeForOffsets(el, m.start, m.end);
      el.classList.add("cm-flash");
      setTimeout(() => el.classList.remove("cm-flash"), 800);
    }
  }

  return activeRange;
}

export function clearHighlights(): void {
  if (isHighlightSupported()) {
    (CSS as any).highlights.delete("cm-find");
    (CSS as any).highlights.delete("cm-find-active");
  }
}
```

- [ ] **Step 2: Type-check**

Run: `(cd crates/webui/frontend && npx tsc --noEmit)`
Expected: PASS (no type errors).

- [ ] **Step 3: Commit**

```bash
git add crates/webui/frontend/src/lib/highlight.ts
git commit -m "feat(webui): DOM range mapping + CSS Custom Highlight painting"
```

---

## Task 5: useFind hook

**Files:**
- Create: `crates/webui/frontend/src/hooks/useFind.ts`

- [ ] **Step 1: Implement `useFind`**

Create `crates/webui/frontend/src/hooks/useFind.ts`:

```ts
import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useState } from "react";
import type { RenderedTurn } from "../types";
import { compileMatcher, type FindOpts } from "../lib/search";
import { exactMatches, compareMatches, type FindMatch } from "../lib/segments";
import { domMarkdownMatches, paintHighlights, clearHighlights } from "../lib/highlight";

const DEFAULT_OPTS: FindOpts = { caseSensitive: false, wholeWord: false, regex: false };

export interface UseFind {
  open: boolean;
  openFind: () => void;
  close: () => void;
  query: string;
  setQuery: (s: string) => void;
  opts: FindOpts;
  setOpts: (o: FindOpts) => void;
  count: number;
  activeIndex: number;
  error: string | null;
  next: () => void;
  prev: () => void;
  revealedToolKeys: Set<string>;
}

export function useFind(containerRef: React.RefObject<HTMLElement | null>, turns: RenderedTurn[]): UseFind {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [opts, setOpts] = useState<FindOpts>(DEFAULT_OPTS);
  const [matches, setMatches] = useState<FindMatch[]>([]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [revealedToolKeys, setRevealedToolKeys] = useState<Set<string>>(new Set());

  const deferredQuery = useDeferredValue(query);

  const openFind = useCallback(() => setOpen(true), []);
  const close = useCallback(() => {
    setOpen(false);
    setQuery("");
    setMatches([]);
    setError(null);
    setRevealedToolKeys(new Set());
    clearHighlights();
  }, []);

  // Reset to the first match whenever the query or options change.
  useEffect(() => {
    setActiveIndex(0);
  }, [deferredQuery, opts]);

  // Recompute matches (exact data-model sources + markdown from the DOM).
  useLayoutEffect(() => {
    if (!open || deferredQuery === "") {
      setMatches([]);
      setError(null);
      clearHighlights();
      return;
    }
    const m = compileMatcher(deferredQuery, opts);
    if (!m.ok) {
      setError(m.error);
      setMatches([]);
      clearHighlights();
      return;
    }
    setError(null);
    const container = containerRef.current;
    const exact = exactMatches(turns, m.find);
    const md = container ? domMarkdownMatches(container, m.find) : [];
    const all = [...exact, ...md].sort(compareMatches);
    setMatches(all);
    setActiveIndex((i) => (all.length === 0 ? 0 : Math.min(i, all.length - 1)));
  }, [open, deferredQuery, opts, turns, containerRef]);

  // Reveal the active match's collapsed tool call so it can be painted.
  useEffect(() => {
    const active = matches[activeIndex];
    if (!active) return;
    if (active.field === "tool-input" || active.field === "tool-result") {
      const key = `${active.turnIndex}:${active.blockIndex}`;
      setRevealedToolKeys((prev) => (prev.has(key) ? prev : new Set(prev).add(key)));
    }
  }, [matches, activeIndex]);

  // Paint + scroll, after any reveal/expand re-render has committed.
  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container || matches.length === 0) {
      clearHighlights();
      return;
    }
    const activeRange = paintHighlights(container, matches, activeIndex);
    activeRange?.startContainer.parentElement?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [matches, activeIndex, revealedToolKeys, containerRef]);

  const next = useCallback(() => {
    setActiveIndex((i) => (matches.length === 0 ? 0 : (i + 1) % matches.length));
  }, [matches.length]);
  const prev = useCallback(() => {
    setActiveIndex((i) => (matches.length === 0 ? 0 : (i - 1 + matches.length) % matches.length));
  }, [matches.length]);

  return {
    open,
    openFind,
    close,
    query,
    setQuery,
    opts,
    setOpts,
    count: matches.length,
    activeIndex,
    error,
    next,
    prev,
    revealedToolKeys,
  };
}
```

- [ ] **Step 2: Type-check**

Run: `(cd crates/webui/frontend && npx tsc --noEmit)`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/webui/frontend/src/hooks/useFind.ts
git commit -m "feat(webui): useFind hook — match compute, navigation, reveal, paint"
```

---

## Task 6: FindBar component

**Files:**
- Create: `crates/webui/frontend/src/components/FindBar.tsx`

- [ ] **Step 1: Implement `FindBar`**

Create `crates/webui/frontend/src/components/FindBar.tsx`:

```tsx
import { useEffect, useRef } from "react";
import type { FindOpts } from "../lib/search";

interface Props {
  query: string;
  setQuery: (s: string) => void;
  opts: FindOpts;
  setOpts: (o: FindOpts) => void;
  count: number;
  activeIndex: number;
  error: string | null;
  onNext: () => void;
  onPrev: () => void;
  onClose: () => void;
}

export default function FindBar({
  query,
  setQuery,
  opts,
  setOpts,
  count,
  activeIndex,
  error,
  onNext,
  onPrev,
  onClose,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const toggle = (key: keyof FindOpts) => setOpts({ ...opts, [key]: !opts[key] });
  const toggleBtn = (active: boolean) =>
    `px-1.5 py-0.5 rounded text-xs font-mono border ${
      active
        ? "border-[var(--color-accent)] text-[var(--color-accent)]"
        : "border-[var(--color-border)] text-[var(--color-text-dim)]"
    } hover:border-[var(--color-accent)] transition-colors`;

  const status = error
    ? error
    : count === 0
      ? query
        ? "No results"
        : ""
      : `${activeIndex + 1} / ${count}`;

  return (
    <div className="absolute top-2 right-4 z-10 flex items-center gap-2 px-2 py-1.5 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] shadow-lg">
      <input
        ref={inputRef}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            if (e.shiftKey) onPrev();
            else onNext();
          } else if (e.key === "Escape") {
            e.preventDefault();
            onClose();
          }
        }}
        placeholder="Find"
        spellCheck={false}
        className="bg-[var(--color-bg)] text-[var(--color-text)] text-sm px-2 py-1 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)] w-48"
      />
      <span
        className={`text-xs tabular-nums min-w-[3.5rem] ${
          error ? "text-[var(--color-error)]" : "text-[var(--color-text-dim)]"
        }`}
      >
        {status}
      </span>
      <button onClick={onPrev} className="text-[var(--color-text-dim)] hover:text-[var(--color-text)] px-1" title="Previous (Shift+Enter)">
        ▲
      </button>
      <button onClick={onNext} className="text-[var(--color-text-dim)] hover:text-[var(--color-text)] px-1" title="Next (Enter)">
        ▼
      </button>
      <span className="w-px self-stretch bg-[var(--color-border)] mx-1" />
      <button onClick={() => toggle("caseSensitive")} className={toggleBtn(opts.caseSensitive)} title="Match case">
        Aa
      </button>
      <button onClick={() => toggle("wholeWord")} className={toggleBtn(opts.wholeWord)} title="Whole word">
        \b
      </button>
      <button onClick={() => toggle("regex")} className={toggleBtn(opts.regex)} title="Regex">
        .*
      </button>
      <button onClick={onClose} className="text-[var(--color-text-dim)] hover:text-[var(--color-text)] px-1" title="Close (Esc)">
        ×
      </button>
    </div>
  );
}
```

- [ ] **Step 2: Type-check**

Run: `(cd crates/webui/frontend && npx tsc --noEmit)`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/webui/frontend/src/components/FindBar.tsx
git commit -m "feat(webui): FindBar UI (input, count, nav, case/word/regex toggles)"
```

---

## Task 7: ToolCall — forceExpand + data-seg-id

**Files:**
- Modify: `crates/webui/frontend/src/components/ToolCall.tsx`

- [ ] **Step 1: Replace the file**

Overwrite `crates/webui/frontend/src/components/ToolCall.tsx` with:

```tsx
import { useState } from "react";
import type { RenderedBlock, ToolCallResult } from "../types";
import { makeSegId } from "../lib/segments";

interface Props {
  block: Extract<RenderedBlock, { type: "tool_call" }>;
  turnIndex: number;
  blockIndex: number;
  forceExpand?: boolean;
}

export default function ToolCall({ block, turnIndex, blockIndex, forceExpand }: Props) {
  const [expanded, setExpanded] = useState(false);
  const open = expanded || !!forceExpand;

  return (
    <div className="my-1">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-baseline text-left w-full hover:bg-[var(--color-surface-hover)] rounded px-2 py-1 transition-colors"
      >
        <span className="text-[var(--color-tool)] w-4 shrink-0 text-center">
          {open ? "▾" : "▸"}
        </span>
        <span
          data-seg-id={makeSegId(turnIndex, blockIndex, "tool-name")}
          className="text-[var(--color-tool)] font-semibold text-sm shrink-0 mr-1.5"
        >
          {block.name}:
        </span>
        <span className="text-[var(--color-text-dim)] text-xs truncate min-w-0">
          {block.input_summary}
        </span>
      </button>

      {open && (
        <div className="ml-4">
          <ToolInput input={block.input_full} segId={makeSegId(turnIndex, blockIndex, "tool-input")} />
          {block.result ? (
            <ToolResult result={block.result} segId={makeSegId(turnIndex, blockIndex, "tool-result")} />
          ) : (
            <div className="px-3 py-1.5 text-xs text-[var(--color-text-dim)] italic">
              (no result)
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function ToolInput({ input, segId }: { input: Record<string, unknown>; segId: string }) {
  return (
    <pre
      data-seg-id={segId}
      className="px-3 py-2 rounded text-xs whitespace-pre-wrap break-words overflow-hidden border-l-2 border-l-[var(--color-tool)] bg-[var(--color-tool-result-bg)] text-[var(--color-text)] mb-1"
      style={{ maxHeight: "300px", overflowY: "auto" }}
    >
      {JSON.stringify(input, null, 2)}
    </pre>
  );
}

function ToolResult({ result, segId }: { result: ToolCallResult; segId: string }) {
  return (
    <pre
      data-seg-id={segId}
      className={`px-3 py-2 rounded text-xs whitespace-pre-wrap break-words overflow-hidden border-l-2 ${
        result.is_error
          ? "border-l-[var(--color-error)] bg-[var(--color-tool-result-bg)] text-[var(--color-error)]"
          : "border-l-[var(--color-border)] bg-[var(--color-tool-result-bg)] text-[var(--color-text-dim)]"
      }`}
      style={{ maxHeight: "400px", overflowY: "auto" }}
    >
      {result.text}
    </pre>
  );
}
```

- [ ] **Step 2: Type-check (expected to fail at the call site)**

Run: `(cd crates/webui/frontend && npx tsc --noEmit)`
Expected: FAIL — `Transcript.tsx` renders `<ToolCall block=... />` without the new required `turnIndex`/`blockIndex` props. This is fixed in Task 9. (If running tasks strictly in order, this is the expected red; proceed to commit the ToolCall change.)

- [ ] **Step 3: Commit**

```bash
git add crates/webui/frontend/src/components/ToolCall.tsx
git commit -m "feat(webui): ToolCall forceExpand + data-seg-id on name/input/result"
```

---

## Task 8: CSS highlight styles

**Files:**
- Modify: `crates/webui/frontend/src/index.css`

- [ ] **Step 1: Append highlight rules**

Add to the end of `crates/webui/frontend/src/index.css`:

```css
/* In-session find highlights (CSS Custom Highlight API) */
::highlight(cm-find) {
  background-color: rgba(210, 153, 34, 0.30);
}
::highlight(cm-find-active) {
  background-color: var(--color-system);
  color: var(--color-bg);
}
/* Fallback flash when the Highlight API is unavailable */
.cm-flash {
  outline: 2px solid var(--color-system);
  outline-offset: 2px;
  border-radius: 2px;
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/webui/frontend/src/index.css
git commit -m "style(webui): find highlight + active-match colors"
```

---

## Task 9: Wire find into Transcript

**Files:**
- Modify: `crates/webui/frontend/src/components/Transcript.tsx`

This threads `turnIndex`/`blockIndex` to `Turn` → `BlockRenderer` → `ToolCall`, adds `data-seg-id` to text segments, suspends compact while find is open, mounts the container ref + `FindBar`, and adds the keyboard handlers.

- [ ] **Step 1: Update imports**

In `crates/webui/frontend/src/components/Transcript.tsx`, change the React import line and add the new imports near the top (after the existing `CommitResultBanner` import):

```tsx
import { useState, useRef, useEffect } from "react";
```

Add:

```tsx
import FindBar from "./FindBar";
import { useFind } from "../hooks/useFind";
import { makeSegId } from "../lib/segments";
```

- [ ] **Step 2: Add ref + hook + keyboard, suspend compact**

Inside `export default function Transcript({ session, onBack }: Props)`, replace the existing top of the function body:

```tsx
  const [statsOpen, setStatsOpen] = useState(true);
  const [compact, setCompact] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  const editor = useEditor();
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const sourcePath = session.source_path;

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [session.turns.length]);

  const { stats } = session;

  const visibleTurns = compact
    ? filterCompact(session.turns)
    : session.turns;
```

with:

```tsx
  const [statsOpen, setStatsOpen] = useState(true);
  const [compact, setCompact] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);
  const turnsRef = useRef<HTMLDivElement>(null);

  const editor = useEditor();
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const sourcePath = session.source_path;

  const find = useFind(turnsRef, session.turns);
  const { openFind, next: findNext, prev: findPrev } = find;

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [session.turns.length]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        openFind();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "g") {
        e.preventDefault();
        if (e.shiftKey) findPrev();
        else findNext();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openFind, findNext, findPrev]);

  const { stats } = session;

  // Suspend compact while find is open so every match is mounted & countable.
  const visibleTurns = compact && !find.open
    ? filterCompact(session.turns)
    : session.turns;
```

- [ ] **Step 3: Make the outer container relative + render FindBar + add the container ref**

Change the outer wrapper opening tag:

```tsx
    <div className="flex flex-col h-full">
```

to:

```tsx
    <div className="flex flex-col h-full relative">
```

Then change the turns scroll container opening tag:

```tsx
      <div className="flex-1 overflow-y-auto overflow-x-hidden px-4 py-4 space-y-4">
        {editor.result && (
```

to:

```tsx
      {find.open && (
        <FindBar
          query={find.query}
          setQuery={find.setQuery}
          opts={find.opts}
          setOpts={find.setOpts}
          count={find.count}
          activeIndex={find.activeIndex}
          error={find.error}
          onNext={find.next}
          onPrev={find.prev}
          onClose={find.close}
        />
      )}
      <div ref={turnsRef} className="flex-1 overflow-y-auto overflow-x-hidden px-4 py-4 space-y-4">
        {editor.result && (
```

- [ ] **Step 4: Pass turnIndex + revealedToolKeys to each Turn**

Change the turns map. Replace:

```tsx
        {visibleTurns.map((turn, i) => {
          const isMessage = turn.type === "user" || turn.type === "assistant";
          const editIndex = isMessage ? turn.edit_index : null;
          return (
            <Turn
              key={i}
              turn={turn}
```

with:

```tsx
        {visibleTurns.map((turn, i) => {
          const isMessage = turn.type === "user" || turn.type === "assistant";
          const editIndex = isMessage ? turn.edit_index : null;
          return (
            <Turn
              key={i}
              turn={turn}
              turnIndex={i}
              revealedToolKeys={find.revealedToolKeys}
```

(The remaining `Turn` props — `editing`, `draft`, `onStartEdit`, etc. — stay exactly as they are.)

Note on indexing: when find is open, `visibleTurns === session.turns`, so the map index `i` equals the real turn index used by the matcher's seg-ids. (When find is closed and compact filters turns, seg-ids are not consulted.)

- [ ] **Step 5: Extend the `Turn` component signature + thread to blocks**

Change the `Turn` function signature. Replace:

```tsx
function Turn({
  turn,
  editing,
  draft,
  onStartEdit,
  onChangeDraft,
  onCancelEdit,
  onSaveEdit,
  onFork,
  onDelete,
}: {
  turn: RenderedTurn;
  editing: boolean;
  draft: string;
  onStartEdit: (text: string) => void;
  onChangeDraft: (s: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
  onFork: () => void;
  onDelete: () => void;
}) {
  const ts = turn.timestamp ? formatTime(turn.timestamp) : "--:--";
```

with:

```tsx
function Turn({
  turn,
  turnIndex,
  revealedToolKeys,
  editing,
  draft,
  onStartEdit,
  onChangeDraft,
  onCancelEdit,
  onSaveEdit,
  onFork,
  onDelete,
}: {
  turn: RenderedTurn;
  turnIndex: number;
  revealedToolKeys: Set<string>;
  editing: boolean;
  draft: string;
  onStartEdit: (text: string) => void;
  onChangeDraft: (s: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
  onFork: () => void;
  onDelete: () => void;
}) {
  const ts = turn.timestamp ? formatTime(turn.timestamp) : "--:--";
```

- [ ] **Step 6: Add data-seg-id to the user text div**

In the `case "user":` branch, replace:

```tsx
            <div className="pl-4 text-sm whitespace-pre-wrap break-words">{turn.text}</div>
```

with:

```tsx
            <div
              data-seg-id={makeSegId(turnIndex, null, "user-text")}
              className="pl-4 text-sm whitespace-pre-wrap break-words"
            >
              {turn.text}
            </div>
```

- [ ] **Step 7: Thread turnIndex/revealedToolKeys into the assistant blocks**

In the `case "assistant":` branch, replace:

```tsx
          <div className="pl-4">
            {turn.blocks.map((block, j) => (
              <BlockRenderer key={j} block={block} />
            ))}
          </div>
```

with:

```tsx
          <div className="pl-4">
            {turn.blocks.map((block, j) => (
              <BlockRenderer
                key={j}
                block={block}
                turnIndex={turnIndex}
                blockIndex={j}
                revealedToolKeys={revealedToolKeys}
              />
            ))}
          </div>
```

- [ ] **Step 8: Add data-seg-id to the system + summary content**

Replace the `case "system":` branch:

```tsx
    case "system":
      return (
        <div className="text-[var(--color-system)] text-sm opacity-70">
          <span className="text-xs">{ts}</span> system: {turn.content}
        </div>
      );
```

with:

```tsx
    case "system":
      return (
        <div className="text-[var(--color-system)] text-sm opacity-70">
          <span className="text-xs">{ts}</span> system:{" "}
          <span data-seg-id={makeSegId(turnIndex, null, "system")}>{turn.content}</span>
        </div>
      );
```

Replace the `case "summary":` branch:

```tsx
    case "summary":
      return (
        <div className="border-t border-b border-[var(--color-border)] py-2 my-2 text-xs text-[var(--color-text-dim)] italic">
          ── compaction summary ──
          <div className="mt-1 whitespace-pre-wrap break-words">{turn.content}</div>
        </div>
      );
```

with:

```tsx
    case "summary":
      return (
        <div className="border-t border-b border-[var(--color-border)] py-2 my-2 text-xs text-[var(--color-text-dim)] italic">
          ── compaction summary ──
          <div
            data-seg-id={makeSegId(turnIndex, null, "summary")}
            className="mt-1 whitespace-pre-wrap break-words"
          >
            {turn.content}
          </div>
        </div>
      );
```

- [ ] **Step 9: Extend `BlockRenderer` to take the indices and add the assistant-text seg-id**

Replace the entire `BlockRenderer` function:

```tsx
function BlockRenderer({ block }: { block: RenderedBlock }) {
  switch (block.type) {
    case "text":
      return (
        <div className="prose prose-sm prose-invert max-w-none mb-2">
          <Markdown components={{ a: ExternalLink }}>{block.text}</Markdown>
        </div>
      );

    case "thinking":
      return null;

    case "tool_call":
      return <ToolCall block={block} />;
  }
}
```

with:

```tsx
function BlockRenderer({
  block,
  turnIndex,
  blockIndex,
  revealedToolKeys,
}: {
  block: RenderedBlock;
  turnIndex: number;
  blockIndex: number;
  revealedToolKeys: Set<string>;
}) {
  switch (block.type) {
    case "text":
      return (
        <div
          data-seg-id={makeSegId(turnIndex, blockIndex, "assistant-text")}
          className="prose prose-sm prose-invert max-w-none mb-2"
        >
          <Markdown components={{ a: ExternalLink }}>{block.text}</Markdown>
        </div>
      );

    case "thinking":
      return null;

    case "tool_call":
      return (
        <ToolCall
          block={block}
          turnIndex={turnIndex}
          blockIndex={blockIndex}
          forceExpand={revealedToolKeys.has(`${turnIndex}:${blockIndex}`)}
        />
      );
  }
}
```

- [ ] **Step 10: Type-check**

Run: `(cd crates/webui/frontend && npx tsc --noEmit)`
Expected: PASS — the ToolCall call-site error from Task 7 is now resolved and all new props line up.

- [ ] **Step 11: Commit**

```bash
git add crates/webui/frontend/src/components/Transcript.tsx
git commit -m "feat(webui): wire in-session find into Transcript (Cmd+F, FindBar, seg-ids)"
```

---

## Task 10: Build + manual verification

**Files:** none (verification only).

- [ ] **Step 1: Full build (tsc + vite)**

Run: `npm --prefix crates/webui/frontend run build`
Expected: PASS — `tsc` clean, `vite build` succeeds.

- [ ] **Step 2: Run the unit tests once more**

Run: `npm --prefix crates/webui/frontend run test`
Expected: PASS — all search + segments tests green.

- [ ] **Step 3: Launch the app and verify behavior**

Run: `make webui` (from repo root).

Verify each (these cannot be unit-tested — they need the real WKWebView Highlight API + layout):

- [ ] `Cmd+F` opens the bar (browser's native find does not appear); `Esc` closes it.
- [ ] Typing highlights all matches; the active match is a distinct color.
- [ ] Count reads `n / total`; `Enter` / `Shift+Enter` move next/prev and wrap; the active match scrolls to center.
- [ ] A match spanning markdown formatting (e.g. search a phrase that crosses a `**bold**` boundary in an assistant message) highlights correctly.
- [ ] Searching text that lives in a **collapsed** tool call: it is counted; navigating to it **auto-expands** that tool call and highlights the hit (try both tool input JSON and a tool result).
- [ ] With **Compact** on, opening find shows all turns; closing find restores compact.
- [ ] `Aa`, `\b`, `.*` toggles change matching; an invalid regex (e.g. `(`) shows the error state with zero matches.
- [ ] Tool input highlight lands on the right characters (offsets map 1:1 into the `<pre>`).

- [ ] **Step 4: Commit (only if Step 3 surfaced fixes)**

```bash
git add -A
git commit -m "fix(webui): in-session find manual-verification fixes"
```

---

## Self-Review

**Spec coverage:**
- Open/close (Cmd+F/Esc) → Task 9 Step 2; FindBar Esc → Task 6.
- Live incremental + ~100ms debounce → `useDeferredValue` in Task 5.
- Match count / No results / error → FindBar `status` (Task 6).
- Next/prev + wrap + scroll-to-center → Task 5 (`next`/`prev`, paint effect scroll) + Task 6 buttons.
- Case / whole-word / regex toggles + invalid-regex error → Task 2 (matcher) + Task 6 (toggles).
- Active vs other match colors → Task 8.
- Coverage: user/assistant/system/summary/tool name/input/result → Task 3 `exactMatches` + Task 4 `domMarkdownMatches`; seg-ids placed in Task 7 + Task 9.
- Thinking excluded → `BlockRenderer` `case "thinking": return null` unchanged (Task 9 Step 9).
- Collapsed tool auto-expand on navigate → Task 5 reveal effect + Task 7 `forceExpand`.
- Compact suspended while open → Task 9 Step 2.
- Live tail recompute + clamp → Task 5 compute effect deps include `turns`; `activeIndex` clamped via `Math.min`.
- Exact-vs-markdown split → Task 3 (exact) + Task 4 (markdown DOM).
- CSS Custom Highlight API + feature-detect + fallback → Task 4 + Task 8.
- Paint in `useLayoutEffect` after reveal commit → Task 5 paint effect keyed on `revealedToolKeys`.
- vitest for the pure matcher → Tasks 1–3.

**Placeholder scan:** none — every code step contains full code; no TBD/TODO.

**Type consistency:** `FindOpts`/`Span`/`Matcher` (search.ts) consumed unchanged by segments.ts, highlight.ts, useFind.ts, FindBar.tsx. `FindMatch`/`SegField`/`makeSegId`/`parseSegId`/`segRank`/`compareMatches`/`exactMatches` (segments.ts) consumed consistently by highlight.ts + useFind.ts. `UseFind` shape returned by `useFind` matches the props threaded into `FindBar` and the `find.*` usages in Transcript. `ToolCall` new required props (`turnIndex`, `blockIndex`, `forceExpand?`) are supplied at its only call site (BlockRenderer, Task 9 Step 9). `revealedToolKeys` key format `` `${turnIndex}:${blockIndex}` `` is identical in useFind (write) and BlockRenderer (read).
