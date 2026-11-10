/** Appearance modes: how much of a transcript a window shows. Each mode is one
 *  complete set of display levels, one per kind of content. Pure; DOM-free. */
import type { RenderedBlock, RenderedTurn } from "../types";
import { toolDiff } from "./toolDiff";

/** How tall an open body may grow. `scroll` is today's fixed box for that kind (see SCROLL_PX). */
export type Height = { kind: "scroll" } | { kind: "clamp"; lines: number } | { kind: "full" };

/** One kind of content: whether it starts shown, and how tall it may be once open. */
export interface Part<Show extends string> {
  show: Show;
  height: Height;
}

export interface AppearanceMode {
  id: string;
  /** Label in the mode dropdown and the jump banner. */
  name: string;
  description: string;
  /** The whole tool row. */
  toolCalls: "shown" | "hidden";
  /** Non-diff input: Bash snippet, question card, JSON. */
  toolInput: Part<"collapsed" | "open">;
  /** Result text and returned images, or "(no result)". */
  toolOutput: Part<"collapsed" | "open">;
  /** Edit/Write/MultiEdit input and a Bash call's file changes. */
  diffs: Part<"collapsed" | "open">;
  taskNotifications: Part<"hidden" | "collapsed" | "open">;
  compactionSummary: Part<"hidden" | "open">;
  /** `!` bash-mode stdout/stderr. */
  shellOutput: Height;
  narration: "shown" | "hidden";
  systemLines: "shown" | "hidden";
}

export const SCROLL_PX = {
  toolInput: 300,
  toolOutput: 400,
  diffs: 300,
  taskNotifications: 400,
  compactionSummary: 400,
  shellOutput: 400,
} as const;

const scroll: Height = { kind: "scroll" };
const full: Height = { kind: "full" };
const clamp = (lines: number): Height => ({ kind: "clamp", lines });

function deepFreeze<T>(o: T): T {
  if (o && typeof o === "object") {
    for (const v of Object.values(o)) deepFreeze(v);
    Object.freeze(o);
  }
  return o;
}

const DEFAULT: AppearanceMode = {
  id: "default",
  name: "Default",
  description: "Tool calls collapsed",
  toolCalls: "shown",
  toolInput: { show: "collapsed", height: scroll },
  toolOutput: { show: "collapsed", height: scroll },
  diffs: { show: "collapsed", height: scroll },
  taskNotifications: { show: "collapsed", height: scroll },
  compactionSummary: { show: "open", height: full },
  shellOutput: scroll,
  narration: "shown",
  systemLines: "shown",
};

/** The five presets, in ⌘⇧D cycle order. */
export const MODES: readonly AppearanceMode[] = deepFreeze([
  DEFAULT,
  {
    ...DEFAULT,
    id: "conversation",
    name: "Conversation",
    description: "Prose only",
    toolCalls: "hidden",
    taskNotifications: { show: "hidden", height: scroll },
    narration: "hidden",
    systemLines: "hidden",
  },
  {
    ...DEFAULT,
    id: "inputs",
    name: "Inputs",
    description: "Tool inputs open",
    toolInput: { show: "open", height: scroll },
    diffs: { show: "open", height: scroll },
  },
  {
    ...DEFAULT,
    id: "truncated",
    name: "Truncated",
    description: "Everything open, clipped to a few lines",
    toolInput: { show: "open", height: clamp(12) },
    toolOutput: { show: "open", height: clamp(15) },
    diffs: { show: "open", height: clamp(30) },
    taskNotifications: { show: "open", height: clamp(15) },
    compactionSummary: { show: "open", height: clamp(20) },
    shellOutput: clamp(15),
  },
  {
    ...DEFAULT,
    id: "everything",
    name: "Everything",
    description: "Everything open, in full",
    toolInput: { show: "open", height: full },
    toolOutput: { show: "open", height: full },
    diffs: { show: "open", height: full },
    taskNotifications: { show: "open", height: full },
    shellOutput: full,
  },
]);

export const DEFAULT_MODE: AppearanceMode = MODES[0];

/** Unknown ids (a hand-edited config, a removed preset) fall back to Default. */
export function modeById(id: string): AppearanceMode {
  return MODES.find((m) => m.id === id) ?? DEFAULT_MODE;
}

export function nextMode(id: string): AppearanceMode {
  const i = MODES.indexOf(modeById(id));
  return MODES[(i + 1) % MODES.length];
}

function blockShown(b: RenderedBlock, mode: AppearanceMode): boolean | null {
  if (b.type === "text") return true;
  if (b.type === "narration") return mode.narration === "shown";
  if (b.type === "tool_call") return mode.toolCalls === "shown";
  return null;
}

/** Whether `mode` can drop any turn at all. */
function hidesKinds(mode: AppearanceMode): boolean {
  return (
    mode.systemLines === "hidden" ||
    mode.taskNotifications.show === "hidden" ||
    mode.compactionSummary.show === "hidden" ||
    mode.narration === "hidden" ||
    mode.toolCalls === "hidden"
  );
}

function keepTurn(t: RenderedTurn, mode: AppearanceMode): boolean {
  switch (t.type) {
    case "system":
      return mode.systemLines === "shown";
    case "task_notification":
      return mode.taskNotifications.show !== "hidden";
    case "summary":
      return mode.compactionSummary.show !== "hidden";
    case "assistant": {
      const shown = t.blocks.map((b) => blockShown(b, mode));
      // A turn this mode hides nothing in (e.g. thinking-only) renders as it does in Default.
      return !shown.includes(false) || shown.includes(true);
    }
    default:
      return true;
  }
}

/** The turns a window renders in `mode`: `turns` itself when nothing drops, else the
 *  original turn objects, since useFind keys on this array's identity. */
export function applyMode(turns: RenderedTurn[], mode: AppearanceMode): RenderedTurn[] {
  return hidesKinds(mode) ? turns.filter((t) => keepTurn(t, mode)) : turns;
}

/** Logical lines in `s`, as find sees the text: its `\n` count + 1. */
export function lineCount(s: string): number {
  let n = 1;
  for (let i = s.indexOf("\n"); i !== -1; i = s.indexOf("\n", i + 1)) n++;
  return n;
}

/** A body box's vertical padding, as CSS lengths. */
export type Pad = { top: string; bottom: string };
const NO_PAD: Pad = { top: "0px", bottom: "0px" };

/** Inline style for a body box at `height`. `scrollPx` is that kind's box in Default. A clamp that overflows
 *  (`over`) drops the bottom padding, which the hidden text would paint into, so the box ends on a line boundary. */
export function boxStyle(
  height: Height,
  scrollPx: number,
  pad: Pad = NO_PAD,
  over = false,
): { maxHeight: string; overflowY?: "auto"; overflow?: "hidden"; paddingBottom?: 0 } | undefined {
  switch (height.kind) {
    case "scroll":
      return { maxHeight: `${scrollPx}px`, overflowY: "auto" };
    case "clamp":
      return over
        ? { maxHeight: `calc(${height.lines} * 1lh + ${pad.top})`, overflow: "hidden", paddingBottom: 0 }
        : { maxHeight: `calc(${height.lines} * 1lh + ${pad.top} + ${pad.bottom})`, overflow: "hidden" };
    case "full":
      return undefined;
  }
}

/** The clamp toggle's label. A clamp cuts wrapped lines, so a count no larger than the clamp would offer less than
 *  is already showing; it is left out then. */
export function clampLabel(lines: number, clampLines: number, lifted: boolean): string {
  if (lifted) return "show less";
  return lines > clampLines ? `show all ${lines} lines` : "show all";
}

/** Which knob governs each part of one tool call. */
export function toolParts(
  mode: AppearanceMode,
  block: Extract<RenderedBlock, { type: "tool_call" }>,
): {
  input: AppearanceMode["toolInput"];
  output: AppearanceMode["toolOutput"];
  changes: AppearanceMode["diffs"] | null;
} {
  return {
    input: toolDiff(block.name, block.input_full) ? mode.diffs : mode.toolInput,
    output: mode.toolOutput,
    changes: block.result?.file_changes ? mode.diffs : null,
  };
}
