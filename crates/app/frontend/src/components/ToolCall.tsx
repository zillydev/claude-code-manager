import { useMemo, useState } from "react";
import type { RenderedBlock, ToolCallResult } from "../types";
import { makeSegId } from "../lib/segments";
import { prettyToolName } from "../lib/toolDisplay";
import { toolInputText, toolResultText } from "../lib/findMatches";
import { diffLineText, diffSources, toolDiff, type DiffLine } from "../lib/toolDiff";
import { highlightToLines, languageForPath, type Token } from "../lib/syntax";
import { askLines, type AskLine } from "../lib/askQuestion";
import { bashLines, type BashLine } from "../lib/bashBody";
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
          <ToolInput
            name={block.name}
            input={block.input_full}
            resultText={block.result?.text ?? null}
            segId={makeSegId(turnIndex, blockIndex, "tool-input")}
          />
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

function ToolInput({
  name,
  input,
  resultText,
  segId,
}: {
  name: string;
  input: Record<string, unknown>;
  /** The tool's result, for AskUserQuestion to mark which option was picked. */
  resultText: string | null;
  segId: string;
}) {
  // Edit/Write/MultiEdit render as a diff, the way Claude Code shows them in
  // the terminal; AskUserQuestion as a question card; everything else keeps the
  // JSON view.
  const diff = toolDiff(name, input);
  const bash = diff ? null : name === "Bash" ? bashLines(input) : null;
  const ask = diff || bash ? null : name === "AskUserQuestion" ? askLines(input, resultText) : null;
  return (
    <pre
      data-seg-id={segId}
      className="px-3 py-2 rounded text-xs whitespace-pre-wrap break-words overflow-hidden border-l-2 border-l-[var(--color-tool)] bg-[var(--color-tool-result-bg)] text-[var(--color-text)] mb-1"
      style={{ maxHeight: "300px", overflowY: "auto" }}
    >
      {diff ? (
        <DiffBody lines={diff} name={name} input={input} />
      ) : bash ? (
        <BashBody lines={bash} />
      ) : ask ? (
        <AskBody lines={ask} />
      ) : (
        toolInputText(name, input, resultText)
      )}
    </pre>
  );
}

/** The diff lines, syntax-highlighted, each tinted across the full width the
 *  way a diff view should be.
 *
 *  Every line still carries its own trailing newline, because the element's
 *  textContent must equal `diffText(lines)` EXACTLY — the find bar paints
 *  model-side matches onto those offsets. That costs nothing visually: a line
 *  break at the END of a block box is trimmed by CSS, so the character lives in
 *  the DOM text without drawing an empty row (verified in a browser — every
 *  line box measures exactly one row tall). */
function DiffBody({
  lines,
  name,
  input,
}: {
  lines: DiffLine[];
  name: string;
  input: Record<string, unknown>;
}) {
  // Highlight each side once, whole: a multi-line string or block comment only
  // parses with its context. Lines then index into their side's tokens.
  const tokens = useMemo(() => {
    const src = diffSources(name, input);
    if (!src) return null;
    const lang = languageForPath(src.path);
    if (!lang) return null;
    return { old: highlightToLines(src.old, lang), new: highlightToLines(src.new, lang) };
  }, [name, input]);

  return (
    <>
      {lines.map((l, i) => {
        const marked = l.kind === "add" || l.kind === "del";
        const side = tokens && l.side ? tokens[l.side][l.index ?? -1] : undefined;
        const tail = i === lines.length - 1 ? "" : "\n";
        return (
          <span
            key={i}
            className={`block ${
              l.kind === "add"
                ? "bg-[color-mix(in_srgb,var(--color-assistant)_14%,transparent)]"
                : l.kind === "del"
                  ? "bg-[color-mix(in_srgb,var(--color-error)_14%,transparent)]"
                  : ""
            }`}
          >
            <span
              className={
                marked
                  ? l.kind === "add"
                    ? "text-[var(--color-assistant)]"
                    : "text-[var(--color-error)]"
                  : "text-[var(--color-text-dim)]"
              }
            >
              {diffLineText({ ...l, text: "" })}
            </span>
            {side && side.length > 0 ? (
              side.map((t: Token, k: number) => (
                <span key={k} className={t.cls}>
                  {t.text}
                </span>
              ))
            ) : (
              <span className={marked ? "" : "text-[var(--color-text-dim)]"}>{l.text}</span>
            )}
            {tail}
          </span>
        );
      })}
    </>
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

/** An AskUserQuestion call as the question card it is, rather than raw JSON:
 *  the question, then its options, with the one the user picked accented.
 *
 *  Same rule as the diff body — each line carries its own newline so the
 *  element's textContent equals `askText(lines)` exactly, and the chosen option
 *  is marked with COLOUR only. A "✓" in the text would shift every find offset
 *  after it, since the matcher never sees the result. */
function AskBody({ lines }: { lines: AskLine[] }) {
  return (
    <>
      {lines.map((l, i) => {
        const picked = !!l.chosen;
        const tail = i === lines.length - 1 ? "" : "\n";
        return (
          <span
            key={i}
            className={`block ${
              l.kind === "question"
                ? "font-medium text-[var(--color-text)]"
                : l.kind === "preview"
                  ? "text-[var(--color-text-dim)] border-l-2 border-[var(--color-border)] ml-4"
                  : picked
                    ? "text-[var(--color-accent)] bg-[color-mix(in_srgb,var(--color-accent)_12%,transparent)]"
                    : "text-[var(--color-text-dim)]"
            }`}
          >
            {l.text}
            {tail}
          </span>
        );
      })}
    </>
  );
}

/** A Bash call as the shell snippet it is: its description and settings as
 *  dim `#` comments, then the script highlighted as bash.
 *
 *  Same rule as the other bodies — each line carries its own newline so the
 *  element's textContent equals `bashText(lines)` exactly, keeping find-bar
 *  offsets aligned. */
function BashBody({ lines }: { lines: BashLine[] }) {
  // Highlight the script as one unit: a heredoc or a quoted block only parses
  // with its neighbours, and half of these calls are multi-line.
  const tokens = useMemo(() => {
    const script = lines
      .filter((l) => l.kind === "cmd")
      .map((l) => l.text)
      .join("\n");
    return highlightToLines(script, "bash");
  }, [lines]);

  let cmdIndex = -1;
  return (
    <>
      {lines.map((l, i) => {
        const tail = i === lines.length - 1 ? "" : "\n";
        if (l.kind === "note") {
          return (
            <span key={i} className="block text-[var(--color-text-dim)] italic">
              {l.text}
              {tail}
            </span>
          );
        }
        cmdIndex += 1;
        const line = tokens[cmdIndex];
        return (
          <span key={i} className="block">
            {line && line.length > 0 ? (
              line.map((t: Token, k: number) => (
                <span key={k} className={t.cls}>
                  {t.text}
                </span>
              ))
            ) : (
              l.text
            )}
            {tail}
          </span>
        );
      })}
    </>
  );
}
