# In-session Find Bar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Zed-style find bar at the top of the transcript — ⌘F opens it, matches are painted in place with the CSS Custom Highlight API, ‹ › / ⌘G step through them, and a toggle decides whether collapsed tool bodies are searched (auto-expanding on navigation).

**Architecture:** Pure matcher (`lib/findMatcher.ts`) + pure model-side match collection (`lib/findMatches.ts`) feed a `useFind` hook that merges them with DOM-side matches (`lib/findPaint.ts`), paints via `CSS.highlights`, and drives reveal-on-navigate through a `revealedKeys` set that `Transcript` plumbs to `ToolCall`/`TaskNotificationBlock` as `forceExpand`. `FindBar.tsx` is a dumb presentational strip.

**Tech Stack:** React 19 + TypeScript, Vite, vitest (pure tests only), Tailwind classes + CSS vars, Lucide icons, CSS Custom Highlight API (WKWebView), `usePersistentState` (localStorage).

**Spec:** `docs/superpowers/specs/2026-08-18-in-session-find-bar-design.md`

## Global Constraints

- Frontend root: `crates/app/frontend/`; run tests with `npm test` (vitest run); typecheck with `npx tsc --noEmit -p .` from that directory.
- Never mutate the transcript DOM for highlighting — painting is via `CSS.highlights` only.
- State setters that run in effects keyed on per-render values must bail when unchanged (functional setter + equality check) — the render-loop guard.
- Tint-only highlights: `color-mix(in srgb, var(--color-system) 35%, transparent)` for all matches, 75% for the active match; never change text colour.
- `::highlight()` rules live raw in `crates/app/frontend/index.html` (lightningcss strips them from `index.css`).
- Icons: Lucide only (`lucide-react`), `size={14} strokeWidth={1.75}` in the bar.
- Commit trailer on every commit: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Work on branch `feat/find-bar`. Do NOT merge to main and do NOT launch the app (`make dev`) — the user verifies and merges.

---

### Task 1: Matcher (`lib/findMatcher.ts`)

**Files:**
- Create: `crates/app/frontend/src/lib/findMatcher.ts`
- Test: `crates/app/frontend/src/lib/findMatcher.test.ts`

**Interfaces:**
- Produces:
  ```ts
  export interface FindOpts { caseSensitive: boolean; wholeWord: boolean; regex: boolean }
  export interface Span { start: number; end: number }
  export type Matcher = { ok: true; find: (text: string) => Span[] } | { ok: false; error: string };
  export function compileMatcher(query: string, opts: FindOpts): Matcher;
  ```

- [ ] **Step 1: Write the failing test**

`crates/app/frontend/src/lib/findMatcher.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { compileMatcher, type FindOpts } from "./findMatcher";

function spans(query: string, opts: Partial<FindOpts>, text: string) {
  const m = compileMatcher(query, { caseSensitive: false, wholeWord: false, regex: false, ...opts });
  if (!m.ok) throw new Error("expected ok matcher");
  return m.find(text);
}

describe("compileMatcher", () => {
  it("plain match is case-insensitive by default", () => {
    expect(spans("foo", {}, "FOO foo Foo")).toHaveLength(3);
  });

  it("respects case-sensitive option", () => {
    expect(spans("foo", { caseSensitive: true }, "FOO foo Foo")).toEqual([{ start: 4, end: 7 }]);
  });

  it("escapes regex metacharacters in plain mode", () => {
    expect(spans("a.b", {}, "axb a.b")).toEqual([{ start: 4, end: 7 }]);
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
    if (!m.ok) expect(m.error.length).toBeGreaterThan(0);
  });

  it("zero-width regex terminates and skips empty matches", () => {
    expect(spans("a*", { regex: true }, "aaa bbb")).toEqual([{ start: 0, end: 3 }]);
  });

  it("empty query yields no matches", () => {
    expect(spans("", {}, "anything")).toEqual([]);
  });

  it("returns matches in ascending order", () => {
    expect(spans("x", {}, "x_x_x").map((m) => m.start)).toEqual([0, 2, 4]);
  });

  it("a matcher can be reused across texts (lastIndex is reset)", () => {
    const m = compileMatcher("ab", { caseSensitive: false, wholeWord: false, regex: false });
    if (!m.ok) throw new Error();
    expect(m.find("ab ab")).toHaveLength(2);
    expect(m.find("ab")).toHaveLength(1);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/app/frontend && npx vitest run src/lib/findMatcher.test.ts`
Expected: FAIL — cannot resolve `./findMatcher`.

- [ ] **Step 3: Write the implementation**

`crates/app/frontend/src/lib/findMatcher.ts`:
```ts
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

function escapeRegExp(s: string): string {
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd crates/app/frontend && npx vitest run src/lib/findMatcher.test.ts`
Expected: PASS (10 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/app/frontend/src/lib/findMatcher.ts crates/app/frontend/src/lib/findMatcher.test.ts
git commit -m "feat(app): find matcher — case / whole-word / regex query compiler

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Model-side matches, ordering, reveal keys (`lib/findMatches.ts`)

**Files:**
- Create: `crates/app/frontend/src/lib/findMatches.ts`
- Test: `crates/app/frontend/src/lib/findMatches.test.ts`
- Modify: `crates/app/frontend/src/components/ToolCall.tsx` (use the shared text transforms)

**Interfaces:**
- Consumes: `Span` from Task 1; `makeSegId`, `SegField` from `lib/segments.ts`; `prettyResultText` from `lib/toolDisplay.ts`; `RenderedTurn`, `ToolCallResult` from `types.ts`.
- Produces:
  ```ts
  export interface FindMatch { segId: string; turnIndex: number; blockIndex: number | null; field: SegField; start: number; end: number }
  export const MODEL_FIELDS: ReadonlySet<SegField>;            // "tool-input" | "tool-result" | "task-result"
  export function parseSegId(segId: string): { turnIndex: number; blockIndex: number | null; field: SegField };
  export function compareMatches(a: FindMatch, b: FindMatch): number;   // document order
  export function toolInputText(input: Record<string, unknown>): string;   // what ToolCall renders
  export function toolResultText(result: ToolCallResult): string;          // what ToolCall renders
  export function modelMatches(turns: RenderedTurn[], find: (t: string) => Span[]): FindMatch[]; // MODEL_FIELDS only
  export function revealKey(m: FindMatch): string | null;   // "ti:bi" for tool bodies, "ti:task" for task results, else null
  export function sameMatches(a: FindMatch[], b: FindMatch[]): boolean;
  ```

- [ ] **Step 1: Write the failing test**

`crates/app/frontend/src/lib/findMatches.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import {
  MODEL_FIELDS,
  compareMatches,
  modelMatches,
  parseSegId,
  revealKey,
  sameMatches,
  toolInputText,
  toolResultText,
  type FindMatch,
} from "./findMatches";
import { compileMatcher } from "./findMatcher";
import { makeSegId } from "./segments";
import type { RenderedTurn } from "../types";

function find(q: string) {
  const m = compileMatcher(q, { caseSensitive: false, wholeWord: false, regex: false });
  if (!m.ok) throw new Error();
  return m.find;
}

const turns: RenderedTurn[] = [
  {
    type: "user", timestamp: null, text: "needle in user text", uuid: "u1",
    edit_index: 0, fork_index: 0, edit_uuid: "u1", fork_uuid: "u1", uuids: ["u1"], editable: true,
  },
  {
    type: "assistant", timestamp: null, model: null, uuid: "a1",
    edit_index: 1, fork_index: 1, edit_uuid: "a1", fork_uuid: "a1", uuids: ["a1"], editable: true,
    blocks: [
      { type: "text", text: "needle in markdown" },
      {
        type: "tool_call", id: "t1", name: "Bash", input_summary: "ls",
        input_full: { command: "ls needle" },
        result: { text: '{"k":"needle"}', is_error: false },
      },
    ],
  },
  { type: "task_notification", timestamp: null, uuid: "n1", status: "done", summary: "needle summary", result: "needle result" },
];

describe("modelMatches", () => {
  it("only searches collapsed-body fields, with the renderer's transforms", () => {
    const ms = modelMatches(turns, find("needle"));
    expect(ms.map((m) => m.field)).toEqual(["tool-input", "tool-result", "task-result"]);
    // tool-input is the pretty-printed JSON, so the offset is inside it
    const input = ms[0];
    expect(toolInputText({ command: "ls needle" }).slice(input.start, input.end)).toBe("needle");
    // tool-result is prettyResultText'd (JSON gets pretty-printed)
    const result = ms[1];
    const pretty = toolResultText({ text: '{"k":"needle"}', is_error: false });
    expect(pretty).toContain("\n"); // it was prettified
    expect(pretty.slice(result.start, result.end)).toBe("needle");
    expect(ms[2]).toMatchObject({ segId: makeSegId(2, null, "task-result"), start: 0, end: 6 });
  });

  it("MODEL_FIELDS names exactly the collapsed-body fields", () => {
    expect([...MODEL_FIELDS].sort()).toEqual(["task-result", "tool-input", "tool-result"]);
  });
});

describe("ordering + keys", () => {
  const mk = (turnIndex: number, blockIndex: number | null, field: FindMatch["field"], start: number): FindMatch => ({
    segId: makeSegId(turnIndex, blockIndex, field), turnIndex, blockIndex, field, start, end: start + 1,
  });

  it("compareMatches sorts by turn, block, field rank, then offset", () => {
    const list = [
      mk(1, 0, "tool-result", 0),
      mk(0, null, "user-text", 5),
      mk(1, 0, "tool-input", 9),
      mk(1, 0, "tool-name", 0),
      mk(0, null, "user-text", 1),
      mk(1, 1, "assistant-text", 0),
    ].sort(compareMatches);
    expect(list.map((m) => `${m.turnIndex}:${m.blockIndex}:${m.field}:${m.start}`)).toEqual([
      "0:null:user-text:1",
      "0:null:user-text:5",
      "1:0:tool-name:0",
      "1:0:tool-input:9",
      "1:0:tool-result:0",
      "1:1:assistant-text:0",
    ]);
  });

  it("parseSegId round-trips makeSegId", () => {
    expect(parseSegId(makeSegId(3, 2, "tool-input"))).toEqual({ turnIndex: 3, blockIndex: 2, field: "tool-input" });
    expect(parseSegId(makeSegId(7, null, "user-text"))).toEqual({ turnIndex: 7, blockIndex: null, field: "user-text" });
  });

  it("revealKey addresses the collapsible that must open for a match", () => {
    expect(revealKey(mk(4, 2, "tool-input", 0))).toBe("4:2");
    expect(revealKey(mk(4, 2, "tool-result", 0))).toBe("4:2");
    expect(revealKey(mk(9, null, "task-result", 0))).toBe("9:task");
    expect(revealKey(mk(9, null, "user-text", 0))).toBeNull();
    expect(revealKey(mk(4, 2, "tool-name", 0))).toBeNull();
  });

  it("sameMatches compares by position, not identity", () => {
    const a = [mk(0, null, "user-text", 1)];
    expect(sameMatches(a, [mk(0, null, "user-text", 1)])).toBe(true);
    expect(sameMatches(a, [mk(0, null, "user-text", 2)])).toBe(false);
    expect(sameMatches(a, [])).toBe(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/app/frontend && npx vitest run src/lib/findMatches.test.ts`
Expected: FAIL — cannot resolve `./findMatches`.

- [ ] **Step 3: Write the implementation**

`crates/app/frontend/src/lib/findMatches.ts`:
```ts
/** In-session find: matches that come from the DATA MODEL (collapsed tool
 *  bodies), plus the ordering/addressing shared with the DOM-side matches
 *  (see findPaint.ts). Pure; DOM-free. */
import type { RenderedTurn, ToolCallResult } from "../types";
import { makeSegId, type SegField } from "./segments";
import { prettyResultText } from "./toolDisplay";
import type { Span } from "./findMatcher";

export interface FindMatch {
  segId: string;
  turnIndex: number;
  blockIndex: number | null;
  field: SegField;
  /** [start, end) offsets into the segment's rendered text. */
  start: number;
  end: number;
}

/** Fields whose text may not be in the DOM (collapsed bodies) — searched from
 *  the model with the renderer's exact transforms, never from the DOM, so a
 *  revealed body is not counted twice. Everything else is DOM-sourced. */
export const MODEL_FIELDS: ReadonlySet<SegField> = new Set<SegField>([
  "tool-input",
  "tool-result",
  "task-result",
]);

export function parseSegId(segId: string): {
  turnIndex: number;
  blockIndex: number | null;
  field: SegField;
} {
  const [t, b, f] = segId.split(":");
  return { turnIndex: Number(t), blockIndex: b === "-" ? null : Number(b), field: f as SegField };
}

// Within one block a tool call renders name → input → result, top to bottom.
const FIELD_RANK: Record<SegField, number> = {
  "user-text": 0,
  "assistant-text": 0,
  system: 0,
  command: 0,
  summary: 0,
  "tool-name": 0,
  "tool-input": 1,
  "tool-result": 2,
  "task-summary": 0,
  "task-result": 1,
};

/** Document order: turn → block → field → offset. */
export function compareMatches(a: FindMatch, b: FindMatch): number {
  return (
    a.turnIndex - b.turnIndex ||
    (a.blockIndex ?? -1) - (b.blockIndex ?? -1) ||
    FIELD_RANK[a.field] - FIELD_RANK[b.field] ||
    a.start - b.start
  );
}

/** The exact text ToolCall renders for a tool's input — one derivation, two
 *  consumers (renderer + find), so painted offsets always line up. */
export function toolInputText(input: Record<string, unknown>): string {
  return JSON.stringify(input, null, 2);
}

/** The exact text ToolCall renders for a tool's result (MCP JSON prettified). */
export function toolResultText(result: ToolCallResult): string {
  return prettyResultText(result.text);
}

/** Matches inside collapsed bodies, read from the model. `turns` must be the
 *  array the transcript actually renders (compact-filtered), since turnIndex
 *  addresses `data-seg-id`s minted from that same array. */
export function modelMatches(turns: RenderedTurn[], find: (t: string) => Span[]): FindMatch[] {
  const out: FindMatch[] = [];
  const push = (turnIndex: number, blockIndex: number | null, field: SegField, text: string) => {
    for (const s of find(text)) {
      out.push({ segId: makeSegId(turnIndex, blockIndex, field), turnIndex, blockIndex, field, start: s.start, end: s.end });
    }
  };
  turns.forEach((turn, ti) => {
    if (turn.type === "task_notification") {
      if (turn.result) push(ti, null, "task-result", turn.result);
    } else if (turn.type === "assistant") {
      turn.blocks.forEach((b, bi) => {
        if (b.type !== "tool_call") return;
        push(ti, bi, "tool-input", toolInputText(b.input_full));
        if (b.result) push(ti, bi, "tool-result", toolResultText(b.result));
      });
    }
  });
  return out;
}

/** Which collapsible must be open for this match to be visible: `"ti:bi"` for
 *  a tool call's body, `"ti:task"` for a task notification's result, null
 *  for always-visible text. Transcript turns these into `forceExpand` props. */
export function revealKey(m: FindMatch): string | null {
  if (m.field === "tool-input" || m.field === "tool-result") return `${m.turnIndex}:${m.blockIndex}`;
  if (m.field === "task-result") return `${m.turnIndex}:task`;
  return null;
}

/** Positional equality, so recompute effects can keep the previous array
 *  (and not re-render) when nothing moved. */
export function sameMatches(a: FindMatch[], b: FindMatch[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const x = a[i], y = b[i];
    if (x.segId !== y.segId || x.start !== y.start || x.end !== y.end) return false;
  }
  return true;
}
```

- [ ] **Step 4: Make ToolCall render through the shared transforms**

In `crates/app/frontend/src/components/ToolCall.tsx`:
- add `import { toolInputText, toolResultText } from "../lib/findMatches";`
- `ToolInput`: replace `{JSON.stringify(input, null, 2)}` with `{toolInputText(input)}`
- `ToolResult`: replace `{prettyResultText(result.text)}` with `{toolResultText(result)}`; remove `prettyResultText` from the `toolDisplay` import (keep `prettyToolName`).

- [ ] **Step 5: Run tests + typecheck**

Run: `cd crates/app/frontend && npx vitest run src/lib/findMatches.test.ts && npx tsc --noEmit -p .`
Expected: PASS (6 tests); tsc clean.

- [ ] **Step 6: Commit**

```bash
git add crates/app/frontend/src/lib/findMatches.ts crates/app/frontend/src/lib/findMatches.test.ts crates/app/frontend/src/components/ToolCall.tsx
git commit -m "feat(app): find matches from the model — tool bodies, ordering, reveal keys

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: DOM side — ranges, DOM matches, painting, scrolling (`lib/findPaint.ts`) + highlight CSS

**Files:**
- Create: `crates/app/frontend/src/lib/findPaint.ts`
- Modify: `crates/app/frontend/index.html` (raw `::highlight()` rules in `<head>`)

**Interfaces:**
- Consumes: `FindMatch`, `MODEL_FIELDS`, `parseSegId` (Task 2); `Span` (Task 1).
- Produces:
  ```ts
  export function isHighlightSupported(): boolean;
  export function rangeForOffsets(el: Element, start: number, end: number): Range | null;
  export function domMatches(container: HTMLElement, find: (t: string) => Span[]): FindMatch[];  // non-MODEL fields, from textContent
  export function paintHighlights(container: HTMLElement, matches: FindMatch[], activeIndex: number): Range | null; // returns active Range if mounted
  export function scrollMatchIntoView(range: Range): void;
  export function clearHighlights(): void;
  ```

No unit test (DOM-only; vitest here has no jsdom). Verified by typecheck + the user's in-app check.

- [ ] **Step 1: Write `findPaint.ts`**

```ts
/** In-session find: the DOM half. Matches for visible segments are read from
 *  the live DOM (`textContent`, so they span markdown/code formatting); all
 *  matches are painted with the CSS Custom Highlight API — no DOM mutation,
 *  so React re-renders can't lose or duplicate marks. */
import type { Span } from "./findMatcher";
import { MODEL_FIELDS, parseSegId, type FindMatch } from "./findMatches";

// The Highlight API has no TS lib typings yet in this toolchain.
type HighlightCtor = new (...ranges: Range[]) => { add(r: Range): void; priority: number };
const highlightCtor = (): HighlightCtor | null =>
  typeof window !== "undefined" && typeof (window as any).Highlight === "function"
    ? ((window as any).Highlight as HighlightCtor)
    : null;
const registry = (): Map<string, unknown> | null =>
  typeof CSS !== "undefined" && (CSS as any).highlights ? ((CSS as any).highlights as Map<string, unknown>) : null;

export function isHighlightSupported(): boolean {
  return highlightCtor() !== null && registry() !== null;
}

/** Map [start, end) offsets within el's concatenated text nodes to a Range. */
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

/** Matches in every visible segment (`[data-seg-id]` whose field is not a
 *  model-sourced collapsed body), read from the rendered text. */
export function domMatches(container: HTMLElement, find: (t: string) => Span[]): FindMatch[] {
  const out: FindMatch[] = [];
  container.querySelectorAll<HTMLElement>("[data-seg-id]").forEach((el) => {
    const segId = el.dataset.segId!;
    const p = parseSegId(segId);
    if (MODEL_FIELDS.has(p.field)) return;
    const text = el.textContent ?? "";
    for (const s of find(text)) {
      out.push({ segId, turnIndex: p.turnIndex, blockIndex: p.blockIndex, field: p.field, start: s.start, end: s.end });
    }
  });
  return out;
}

/** Paint `cm-find` (every mounted non-active match) and `cm-find-active`.
 *  Matches whose segment is not mounted (a still-collapsed body) are skipped.
 *  Returns the active match's Range when it is mounted, for scrolling. */
export function paintHighlights(container: HTMLElement, matches: FindMatch[], activeIndex: number): Range | null {
  const byId = new Map<string, Element>();
  container.querySelectorAll<HTMLElement>("[data-seg-id]").forEach((el) => byId.set(el.dataset.segId!, el));

  let activeRange: Range | null = null;
  const Ctor = highlightCtor();
  const reg = registry();
  const rest = Ctor ? new Ctor() : null;

  matches.forEach((m, i) => {
    const el = byId.get(m.segId);
    if (!el) return;
    const r = rangeForOffsets(el, m.start, m.end);
    if (!r) return;
    if (i === activeIndex) activeRange = r;
    else rest?.add(r);
  });

  if (Ctor && reg && rest) {
    reg.set("cm-find", rest);
    if (activeRange) {
      const act = new Ctor(activeRange);
      act.priority = 1;
      reg.set("cm-find-active", act);
    } else {
      reg.delete("cm-find-active");
    }
  }
  return activeRange;
}

/** Centre the matched text in every scrollable ancestor, innermost first —
 *  a match inside a tool-result <pre> (its own overflow-y) needs the <pre>
 *  scrolled AND the transcript scrolled. Recomputes the rect after each step. */
export function scrollMatchIntoView(range: Range): void {
  let el: Element | null =
    range.startContainer.nodeType === Node.ELEMENT_NODE
      ? (range.startContainer as Element)
      : range.startContainer.parentElement;
  while (el) {
    const style = getComputedStyle(el);
    const scrollableY = /(auto|scroll|overlay)/.test(style.overflowY) && el.scrollHeight > el.clientHeight;
    if (scrollableY) {
      const matchRect = range.getBoundingClientRect();
      const elRect = el.getBoundingClientRect();
      el.scrollTop += matchRect.top + matchRect.height / 2 - (elRect.top + el.clientHeight / 2);
    }
    el = el.parentElement;
  }
}

export function clearHighlights(): void {
  const reg = registry();
  if (!reg) return;
  reg.delete("cm-find");
  reg.delete("cm-find-active");
}
```

- [ ] **Step 2: Add the highlight rules to `index.html`**

Replace the `<head>` of `crates/app/frontend/index.html` with:
```html
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Claude Code Manager</title>
    <!--
      In-session find highlights. Served raw here (not via the Tailwind/
      lightningcss pipeline) because lightningcss does not recognise the
      ::highlight() pseudo-element and strips the rules. Tint only — the text
      colour is left alone so matches stay readable in both themes (the
      global find's cards use the same 35% tint).
    -->
    <style>
      ::highlight(cm-find) {
        background-color: color-mix(in srgb, var(--color-system, #d29922) 35%, transparent);
      }
      ::highlight(cm-find-active) {
        background-color: color-mix(in srgb, var(--color-system, #d29922) 75%, transparent);
      }
    </style>
  </head>
```

- [ ] **Step 3: Typecheck**

Run: `cd crates/app/frontend && npx tsc --noEmit -p .`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/app/frontend/src/lib/findPaint.ts crates/app/frontend/index.html
git commit -m "feat(app): find painting — DOM matches, CSS highlights, nested-scroll reveal

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: `useFind` hook

**Files:**
- Create: `crates/app/frontend/src/hooks/useFind.ts`

**Interfaces:**
- Consumes: Tasks 1–3; `usePersistentState` from `hooks/usePersistentState.ts`.
- Produces:
  ```ts
  export interface FindOptions extends FindOpts { tools: boolean }   // tools = include collapsed tool calls/results
  export interface UseFind {
    open: boolean;
    /** Open (or refocus) the bar. `prefill` non-empty replaces the query. */
    openBar: (prefill?: string | null) => void;
    close: () => void;
    query: string; setQuery: (s: string) => void;
    opts: FindOptions; setOpts: (o: FindOptions) => void;
    count: number; activeIndex: number; error: string | null;
    next: () => void; prev: () => void;
    /** Bumps on every openBar() so the bar can focus+select its input. */
    focusNonce: number;
    /** Collapsibles forced open for navigation — see revealKey(). */
    revealedKeys: ReadonlySet<string>;
  }
  export function useFind(containerRef: React.RefObject<HTMLElement | null>, turns: RenderedTurn[]): UseFind;
  ```

- [ ] **Step 1: Write the hook**

`crates/app/frontend/src/hooks/useFind.ts`:
```ts
import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useState } from "react";
import type { RenderedTurn } from "../types";
import { compileMatcher, type FindOpts } from "../lib/findMatcher";
import { compareMatches, modelMatches, revealKey, sameMatches, type FindMatch } from "../lib/findMatches";
import { clearHighlights, domMatches, paintHighlights, scrollMatchIntoView } from "../lib/findPaint";
import { usePersistentState } from "./usePersistentState";

export interface FindOptions extends FindOpts {
  /** Include collapsed tool calls / results / task results (auto-expanding on navigation). */
  tools: boolean;
}

const DEFAULT_OPTS: FindOptions = { caseSensitive: false, wholeWord: false, regex: false, tools: false };
const EMPTY_KEYS: ReadonlySet<string> = new Set();

export interface UseFind {
  open: boolean;
  openBar: (prefill?: string | null) => void;
  close: () => void;
  query: string;
  setQuery: (s: string) => void;
  opts: FindOptions;
  setOpts: (o: FindOptions) => void;
  count: number;
  activeIndex: number;
  error: string | null;
  next: () => void;
  prev: () => void;
  focusNonce: number;
  revealedKeys: ReadonlySet<string>;
}

/**
 * In-session find state. `turns` MUST be the array the transcript renders
 * (compact-filtered) and MUST be referentially stable across renders
 * (useMemo) — the recompute effect keys on it. Visible text is matched off the
 * DOM inside `containerRef`; collapsed tool bodies off the model when
 * `opts.tools` is on. Painting happens in a layout effect AFTER any reveal
 * has committed, so an auto-expanded body is highlighted the frame it appears.
 */
export function useFind(containerRef: React.RefObject<HTMLElement | null>, turns: RenderedTurn[]): UseFind {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  // Zed-style: the toggles are a user preference, shared across sessions/windows.
  const [opts, setOpts] = usePersistentState<FindOptions>("find.opts", DEFAULT_OPTS);
  const [matches, setMatches] = useState<FindMatch[]>([]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [revealedKeys, setRevealedKeys] = useState<ReadonlySet<string>>(EMPTY_KEYS);
  const [focusNonce, setFocusNonce] = useState(0);

  const deferredQuery = useDeferredValue(query);

  const openBar = useCallback((prefill?: string | null) => {
    setOpen(true);
    if (prefill) setQuery(prefill);
    setFocusNonce((n) => n + 1);
  }, []);

  // Close keeps the query (reopen shows the last search, selected) but drops
  // everything derived: matches, error, reveals, paint.
  const close = useCallback(() => {
    setOpen(false);
    setMatches((prev) => (prev.length === 0 ? prev : []));
    setError(null);
    setRevealedKeys((prev) => (prev.size === 0 ? prev : EMPTY_KEYS));
    clearHighlights();
  }, []);

  // A new query or option set starts from the first match.
  useEffect(() => {
    setActiveIndex(0);
  }, [deferredQuery, opts]);

  // Switching the tools toggle off collapses what navigation had opened.
  useEffect(() => {
    if (!opts.tools) setRevealedKeys((prev) => (prev.size === 0 ? prev : EMPTY_KEYS));
  }, [opts.tools]);

  // Recompute matches. Layout effect so the DOM for `turns` has committed.
  // Functional setters that bail when unchanged: this keys on `turns`, and an
  // unconditional fresh array here would loop render → effect → setState.
  useLayoutEffect(() => {
    if (!open || deferredQuery === "") {
      setMatches((prev) => (prev.length === 0 ? prev : []));
      setError(null);
      clearHighlights();
      return;
    }
    const m = compileMatcher(deferredQuery, opts);
    if (!m.ok) {
      setError(m.error);
      setMatches((prev) => (prev.length === 0 ? prev : []));
      clearHighlights();
      return;
    }
    setError(null);
    const container = containerRef.current;
    const dom = container ? domMatches(container, m.find) : [];
    const model = opts.tools ? modelMatches(turns, m.find) : [];
    const all = [...dom, ...model].sort(compareMatches);
    setMatches((prev) => (sameMatches(prev, all) ? prev : all));
    setActiveIndex((i) => (all.length === 0 ? 0 : Math.min(i, all.length - 1)));
  }, [open, deferredQuery, opts, turns, containerRef]);

  // Stepping onto a match inside a collapsed body opens that body.
  useEffect(() => {
    const key = matches[activeIndex] ? revealKey(matches[activeIndex]) : null;
    if (!key) return;
    setRevealedKeys((prev) => (prev.has(key) ? prev : new Set(prev).add(key)));
  }, [matches, activeIndex]);

  // Paint + scroll — after the reveal above has committed (keyed on it).
  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!open || !container || matches.length === 0) {
      clearHighlights();
      return;
    }
    const activeRange = paintHighlights(container, matches, activeIndex);
    if (activeRange) scrollMatchIntoView(activeRange);
  }, [open, matches, activeIndex, revealedKeys, containerRef]);

  const next = useCallback(() => {
    setActiveIndex((i) => (matches.length === 0 ? 0 : (i + 1) % matches.length));
  }, [matches.length]);
  const prev = useCallback(() => {
    setActiveIndex((i) => (matches.length === 0 ? 0 : (i - 1 + matches.length) % matches.length));
  }, [matches.length]);

  // ⌘G / ⌘⇧G step matches while the bar is open, wherever focus is.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey || e.key.toLowerCase() !== "g") return;
      e.preventDefault();
      e.stopPropagation();
      if (e.shiftKey) prev();
      else next();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, next, prev]);

  return {
    open, openBar, close,
    query, setQuery,
    opts, setOpts,
    count: matches.length, activeIndex, error,
    next, prev,
    focusNonce,
    revealedKeys,
  };
}
```

- [ ] **Step 2: Typecheck**

Run: `cd crates/app/frontend && npx tsc --noEmit -p .`
Expected: clean. (`usePersistentState` returns `[T, Dispatch<SetStateAction<T>>]`; passing it as `setOpts: (o: FindOptions) => void` is assignable.)

- [ ] **Step 3: Commit**

```bash
git add crates/app/frontend/src/hooks/useFind.ts
git commit -m "feat(app): useFind — in-session find state, reveal-on-navigate, paint

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: `FindBar` component

**Files:**
- Create: `crates/app/frontend/src/components/FindBar.tsx`

**Interfaces:**
- Consumes: `FindOptions` (Task 4).
- Produces:
  ```ts
  interface Props {
    query: string; setQuery: (s: string) => void;
    opts: FindOptions; setOpts: (o: FindOptions) => void;
    count: number; activeIndex: number; error: string | null;
    focusNonce: number;
    onNext: () => void; onPrev: () => void; onClose: () => void;
    /** ↗ hand the query to the global find pinned to this session. */
    onSearchAll: () => void;
  }
  export default function FindBar(props: Props): JSX.Element;
  ```

- [ ] **Step 1: Write the component**

`crates/app/frontend/src/components/FindBar.tsx`:
```tsx
import { useEffect, useRef } from "react";
import {
  CaseSensitive,
  ChevronLeft,
  ChevronRight,
  Globe,
  Regex,
  WholeWord,
  Wrench,
  X,
} from "lucide-react";
import type { FindOptions } from "../hooks/useFind";

interface Props {
  query: string;
  setQuery: (s: string) => void;
  opts: FindOptions;
  setOpts: (o: FindOptions) => void;
  count: number;
  activeIndex: number;
  error: string | null;
  /** Bumps whenever the bar should (re)take focus and select its text. */
  focusNonce: number;
  onNext: () => void;
  onPrev: () => void;
  onClose: () => void;
  /** ↗ hand the query to the global find pinned to this session. */
  onSearchAll: () => void;
}

const ICON = { size: 14, strokeWidth: 1.75 } as const;

/** Zed-style buffer-search strip: query · Aa/word/regex · tools · ‹ › n/N · ↗ ✕.
 *  Presentational — all state lives in useFind. */
export default function FindBar({
  query, setQuery, opts, setOpts, count, activeIndex, error, focusNonce,
  onNext, onPrev, onClose, onSearchAll,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [focusNonce]);

  const toggle = (key: keyof FindOptions) => setOpts({ ...opts, [key]: !opts[key] });
  // Same active/inactive treatment as the sidebar tab strip.
  const toggleCls = (active: boolean) =>
    `flex items-center justify-center rounded px-1.5 py-1 transition-colors ${
      active
        ? "bg-[var(--color-surface-hover)] text-[var(--color-accent)]"
        : "text-[var(--color-text-dim)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-hover)]"
    }`;
  const plainCls =
    "flex items-center justify-center rounded px-1.5 py-1 text-[var(--color-text-dim)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-hover)] transition-colors disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-[var(--color-text-dim)]";
  const sep = <span className="w-px self-stretch bg-[var(--color-border)] mx-1" aria-hidden />;

  return (
    <div className="flex items-center gap-1 px-3 py-1.5 border-b border-[var(--color-border)] bg-[var(--color-surface)] shrink-0">
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
        placeholder="Find in session"
        spellCheck={false}
        aria-label="Find in session"
        className="flex-1 min-w-0 bg-[var(--color-bg)] text-[var(--color-text)] text-sm px-2 py-1 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
      />

      <button onClick={() => toggle("caseSensitive")} className={toggleCls(opts.caseSensitive)} title="Match case" aria-pressed={opts.caseSensitive}>
        <CaseSensitive {...ICON} />
      </button>
      <button onClick={() => toggle("wholeWord")} className={toggleCls(opts.wholeWord)} title="Whole word" aria-pressed={opts.wholeWord}>
        <WholeWord {...ICON} />
      </button>
      <button onClick={() => toggle("regex")} className={toggleCls(opts.regex)} title="Regular expression" aria-pressed={opts.regex}>
        <Regex {...ICON} />
      </button>

      {sep}
      <button
        onClick={() => toggle("tools")}
        className={toggleCls(opts.tools)}
        title="Include tool calls & results (expands them when navigated to)"
        aria-pressed={opts.tools}
      >
        <Wrench {...ICON} />
      </button>

      {sep}
      <button onClick={onPrev} disabled={count === 0} className={plainCls} title="Previous match (⇧Enter · ⌘⇧G)">
        <ChevronLeft {...ICON} />
      </button>
      <button onClick={onNext} disabled={count === 0} className={plainCls} title="Next match (Enter · ⌘G)">
        <ChevronRight {...ICON} />
      </button>
      <span
        className={`text-xs tabular-nums min-w-[3.5rem] text-center ${
          error ? "text-[var(--color-error)]" : "text-[var(--color-text-dim)]"
        }`}
        title={error ?? undefined}
      >
        {error ? "Invalid regex" : count === 0 ? "0/0" : `${activeIndex + 1}/${count}`}
      </span>

      {sep}
      <button onClick={onSearchAll} className={plainCls} title="Search all sessions with this query (global find, pinned to this session)">
        <Globe {...ICON} />
      </button>
      <button onClick={onClose} className={plainCls} title="Close (Esc)">
        <X {...ICON} />
      </button>
    </div>
  );
}
```

- [ ] **Step 2: Typecheck**

Run: `cd crates/app/frontend && npx tsc --noEmit -p .`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/app/frontend/src/components/FindBar.tsx
git commit -m "feat(app): FindBar — Zed-style strip (query, toggles, tools, ‹ › n/N, ↗, ✕)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: Wire it into Transcript + App (⌘F, bar mount, reveal plumbing, ↗ bridge)

**Files:**
- Modify: `crates/app/frontend/src/components/Transcript.tsx`
- Modify: `crates/app/frontend/src/App.tsx` (the `onOpenFind` prop, currently line ~373)

**Interfaces:**
- Consumes: `useFind` (Task 4), `FindBar` (Task 5), `revealKey` semantics (Task 2: `"ti:bi"` / `"ti:task"`).
- Produces: `Transcript` prop `onOpenFind?: (sessionId: string, query: string) => void`.

- [ ] **Step 1: Imports + prop type**

In `Transcript.tsx`:
- add imports: `import FindBar from "./FindBar";` and `import { useFind } from "../hooks/useFind";` (next to the other component/hook imports).
- change the prop doc + type:
  ```ts
  /** ↗ in the find bar: open the global find pinned to this session with `query`. */
  onOpenFind?: (sessionId: string, query: string) => void;
  ```

- [ ] **Step 2: Memoise `visibleTurns` and create the hook**

Replace
```ts
  const visibleTurns = compact ? filterCompact(session.turns) : session.turns;
```
with
```ts
  // MUST be memoised: useFind's recompute effect keys on this list and sets
  // state — a per-render identity would loop render → effect → setState.
  const visibleTurns = useMemo(
    () => (compact ? filterCompact(session.turns) : session.turns),
    [compact, session.turns],
  );
  // In-session find (⌘F). Matches what is rendered — compact mode included.
  const find = useFind(turnsRef, visibleTurns);
  const { open: findOpen, openBar: findOpenBar, close: findClose } = find;
```
(`useMemo` is already imported from react in this file — verify; add it if not.)

- [ ] **Step 3: Replace the ⌘F handler**

Replace the existing effect
```ts
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        // Same UI as ⌘⇧F, pinned to this session by id.
        onOpenFind?.(session.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onOpenFind, session.id]);
```
with
```ts
  // ⌘F opens/refocuses the in-session find bar (Zed's buffer search; ⌘⇧F is
  // the global find, handled in App). Only while this tab is showing — with
  // the Terminal tab up, ⌘F would otherwise open a bar nobody can see. A text
  // selection inside the transcript seeds the query, as in Zed.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey || e.key.toLowerCase() !== "f") return;
      if (!visible) return;
      e.preventDefault();
      const sel = window.getSelection();
      const inTranscript = !!sel && !!sel.anchorNode && !!turnsRef.current?.contains(sel.anchorNode);
      const text = inTranscript ? sel!.toString().trim() : "";
      findOpenBar(text && !text.includes("\n") ? text : null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [visible, findOpenBar]);

  // A different session means different turn indices: drop the bar's state.
  useEffect(() => {
    findClose();
  }, [session.id, findClose]);
```

- [ ] **Step 4: Mount the bar above the turns**

Directly before the `{/* Turns */}` comment (after the stats panel block), insert:
```tsx
      {findOpen && (
        <FindBar
          query={find.query}
          setQuery={find.setQuery}
          opts={find.opts}
          setOpts={find.setOpts}
          count={find.count}
          activeIndex={find.activeIndex}
          error={find.error}
          focusNonce={find.focusNonce}
          onNext={find.next}
          onPrev={find.prev}
          onClose={find.close}
          onSearchAll={() => onOpenFind?.(session.id, find.query)}
        />
      )}
```

- [ ] **Step 5: Plumb `revealedKeys` → `forceExpand`**

1. In the `visibleTurns.map(...)` render, pass `revealedKeys={find.revealedKeys}` to `<Turn …/>`.
2. `Turn` (memo): add prop `revealedKeys: ReadonlySet<string>;` to its props type and destructuring. In the `"assistant"` case pass to `BlockRenderer`: `forceExpand={revealedKeys.has(`${turnIndex}:${j}`)}`. In the `"task_notification"` case pass `forceExpand={revealedKeys.has(`${turnIndex}:task`)}` to `TaskNotificationBlock`.
3. `BlockRenderer` (memo): add `forceExpand: boolean` to its props and pass `forceExpand={forceExpand}` to `<ToolCall …/>` in the `tool_call` case. (`ToolCall` already accepts `forceExpand?: boolean`; `TaskNotificationBlock` already accepts `forceExpand?: boolean`.)

- [ ] **Step 6: App wiring**

In `crates/app/frontend/src/App.tsx` change
```tsx
                    onOpenFind={(id) => openFindTab(`session:${id} `)}
```
to
```tsx
                    onOpenFind={(id, q) => openFindTab(q ? `session:${id} ${q}` : `session:${id} `)}
```

- [ ] **Step 7: Typecheck + full test suite**

Run: `cd crates/app/frontend && npx tsc --noEmit -p . && npm test`
Expected: tsc clean; all vitest suites pass.

- [ ] **Step 8: Commit**

```bash
git add crates/app/frontend/src/components/Transcript.tsx crates/app/frontend/src/App.tsx
git commit -m "feat(app): ⌘F opens the in-session find bar; ↗ bridges to the global find

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: Hand-off for verification (no code)

- [ ] Report to the user: what to try (⌘F, type, Enter/⇧Enter, ⌘G/⌘⇧G, toggles incl. tools auto-expand, ↗, Esc, ⌘F with a selection, Terminal tab ⌘F no-op, compact mode) and hold on `feat/find-bar` until they say merge. On merge: `FEATURES.md` gets an "In-session find (⌘F)" entry replacing the current "In-session find (⌘F) — opens the global find pinned…" line, and the "Global find" line notes ⌘⇧F / ↗ from the bar.
