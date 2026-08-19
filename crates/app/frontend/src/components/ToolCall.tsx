import { useState } from "react";
import type { RenderedBlock, ToolCallResult } from "../types";
import { makeSegId } from "../lib/segments";
import { prettyToolName } from "../lib/toolDisplay";
import { toolInputText, toolResultText } from "../lib/findMatches";
import { ImageStrip } from "./InlineImage";

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
          {prettyToolName(block.name)}:
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
      {toolInputText(input)}
    </pre>
  );
}

function ToolResult({ result, segId }: { result: ToolCallResult; segId: string }) {
  return (
    <div
      className={`px-3 py-2 rounded text-xs border-l-2 ${
        result.is_error
          ? "border-l-[var(--color-error)] bg-[var(--color-tool-result-bg)] text-[var(--color-error)]"
          : "border-l-[var(--color-border)] bg-[var(--color-tool-result-bg)] text-[var(--color-text-dim)]"
      }`}
      style={{ maxHeight: "400px", overflowY: "auto" }}
    >
      <pre data-seg-id={segId} className="whitespace-pre-wrap break-words overflow-hidden">
        {toolResultText(result)}
      </pre>
      <ImageStrip images={result.images} label="tool result image" />
    </div>
  );
}
