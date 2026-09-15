import { prettyToolName } from "../lib/toolDisplay";
import type { ToolPreview } from "../types";

/** A tool call as one preview line: its name styled as the transcript's tool header, then its key input. */
export default function ToolPreviewLine({ tool }: { tool: ToolPreview }) {
  return (
    <>
      <span className="text-[var(--color-tool)] font-semibold mr-1.5">{prettyToolName(tool.name)}:</span>
      <span>{tool.summary}</span>
    </>
  );
}
