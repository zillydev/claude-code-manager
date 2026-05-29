import { useState } from "react";
import type { RenderedBlock, ToolCallResult } from "../types";

interface Props {
  block: Extract<RenderedBlock, { type: "tool_call" }>;
}

export default function ToolCall({ block }: Props) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="my-1">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-baseline text-left w-full hover:bg-[var(--color-surface-hover)] rounded px-2 py-1 transition-colors"
      >
        <span className="text-[var(--color-tool)] w-4 shrink-0 text-center">
          {expanded ? "▾" : "▸"}
        </span>
        <span className="text-[var(--color-tool)] font-semibold text-sm shrink-0 mr-1.5">
          {block.name}:
        </span>
        <span className="text-[var(--color-text-dim)] text-xs truncate min-w-0">
          {block.input_summary}
        </span>
      </button>

      {expanded && (
        <div className="ml-4">
          <ToolInput input={block.input_full} />
          {block.result ? (
            <ToolResult result={block.result} />
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

function ToolInput({ input }: { input: Record<string, unknown> }) {
  return (
    <pre
      className="px-3 py-2 rounded text-xs whitespace-pre-wrap break-words overflow-hidden border-l-2 border-l-[var(--color-tool)] bg-[var(--color-tool-result-bg)] text-[var(--color-text)] mb-1"
      style={{ maxHeight: "300px", overflowY: "auto" }}
    >
      {JSON.stringify(input, null, 2)}
    </pre>
  );
}

function ToolResult({ result }: { result: ToolCallResult }) {
  return (
    <pre
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
