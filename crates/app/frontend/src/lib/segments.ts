import type { RenderedTurn } from "../types";

export type SegField =
  | "user-text"
  | "assistant-text"
  | "narration"
  | "system"
  | "command"
  | "summary"
  | "tool-name"
  | "tool-input"
  | "tool-result"
  | "tool-changes"
  | "task-summary"
  | "task-result";

/** Stable id for an addressable text segment of a rendered turn. Emitted as
 *  `data-seg-id` on transcript nodes so features can locate a segment's DOM. */
export function makeSegId(turnIndex: number, blockIndex: number | null, field: SegField): string {
  return `${turnIndex}:${blockIndex ?? "-"}:${field}`;
}

// Referenced so the type stays anchored to the shape it describes.
export type { RenderedTurn as _SegTurn };
