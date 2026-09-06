/** In-session find: matches that come from the DATA MODEL (collapsed tool
 *  bodies), plus the ordering/addressing shared with the DOM-side matches
 *  (see findPaint.ts). Pure; DOM-free. */
import type { RenderedTurn, ToolCallResult } from "../types";
import { makeSegId, type SegField } from "./segments";
import { prettyResultText } from "./toolDisplay";
import { diffText, toolDiff } from "./toolDiff";
import { askLines, askText } from "./askQuestion";
import { bashLines, bashText } from "./bashBody";
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
  narration: 0,
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
 *  consumers (renderer + find), so painted offsets always line up. File-editing
 *  tools render as a diff, Bash as a shell snippet, AskUserQuestion as its
 *  question card; everything else as pretty JSON. */
export function toolInputText(name: string, input: Record<string, unknown>): string {
  const d = toolDiff(name, input);
  if (d) return diffText(d);
  if (name === "Bash") {
    const lines = bashLines(input);
    if (lines) return bashText(lines);
  }
  if (name === "AskUserQuestion") {
    const lines = askLines(input);
    if (lines) return askText(lines);
  }
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
        push(ti, bi, "tool-input", toolInputText(b.name, b.input_full));
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
