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
