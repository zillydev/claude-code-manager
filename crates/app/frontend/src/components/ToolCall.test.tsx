import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import ToolCall from "./ToolCall";
import type { RenderedBlock } from "../types";
import { DEFAULT_MODE, modeById } from "../lib/appearance";
import { toolResultText } from "../lib/findMatches";

type ToolBlock = Extract<RenderedBlock, { type: "tool_call" }>;

const call = (name: string, input: Record<string, unknown>, result: ToolBlock["result"]): ToolBlock => ({
  type: "tool_call",
  id: name,
  name,
  input_summary: "summary",
  input_full: input,
  result,
});

const cases: Record<string, ToolBlock> = {
  bash: call("Bash", { command: "ls -la\necho done", description: "list" }, {
    text: "\u001b[32mok\u001b[0m\nline two",
    is_error: false,
  }),
  edit: call("Edit", { file_path: "/repo/a.ts", old_string: "const a = 1;", new_string: "const a = 2;" }, {
    text: "updated",
    is_error: false,
  }),
  mcpJson: call("mcp__db__run_query", { sql: "select 1" }, { text: '{"rows":[{"n":1}]}', is_error: false }),
  error: call("Read", { file_path: "/nope" }, { text: "File does not exist.", is_error: true }),
  noResult: call("Grep", { pattern: "x" }, null),
  fileChanges: call("Bash", { command: "sed -i s/a/b/ f.txt" }, {
    text: "",
    is_error: false,
    file_changes: {
      files: [
        {
          path: "/repo/f.txt",
          created: false,
          hunks: [{ old_start: 1, old_lines: 1, new_start: 1, new_lines: 1, lines: ["-a", "+b"] }],
        },
      ],
      more: 0,
    },
  }),
  images: call("Screenshot", {}, {
    text: "captured",
    is_error: false,
    images: [{ media_type: "image/png", data: "iVBORw0KGgo=" }],
  }),
};

// Default mode must render today's markup byte for byte; these snapshots pin it.
describe("ToolCall markup", () => {
  for (const [name, block] of Object.entries(cases)) {
    it(`${name}, collapsed`, () => {
      expect(renderToStaticMarkup(<ToolCall block={block} turnIndex={3} blockIndex={1} mode={DEFAULT_MODE} modeSeq={0} />)).toMatchSnapshot();
    });
    it(`${name}, expanded`, () => {
      expect(
        renderToStaticMarkup(<ToolCall block={block} turnIndex={3} blockIndex={1} mode={DEFAULT_MODE} modeSeq={0} forceExpand />),
      ).toMatchSnapshot();
    });
  }
});

const long = call("Bash", { command: "seq 20" }, {
  text: Array.from({ length: 20 }, (_, i) => `line ${i + 1}`).join("\n"),
  is_error: false,
});
const render = (modeId: string, forceExpand = false) =>
  renderToStaticMarkup(
    <ToolCall block={long} turnIndex={3} blockIndex={1} mode={modeById(modeId)} modeSeq={0} forceExpand={forceExpand} />,
  );
const unescape = (html: string) =>
  html.replace(/<[^>]*>/g, "").replace(/&quot;/g, '"').replace(/&#x27;/g, "'").replace(/&amp;/g, "&");

describe("ToolCall in other modes", () => {
  it("Truncated clamps the output box with CSS and keeps the whole text in the find segment", () => {
    const html = render("truncated");
    const seg = html.match(/<pre data-seg-id="3:1:tool-result"[^>]*>([\s\S]*?)<\/pre>/)!;
    expect(unescape(seg[1])).toBe(toolResultText(long.result!));
    expect(html).toContain("max-height:calc(15 * 1lh + 0.5rem);overflow:hidden;padding-bottom:0");
    expect(html).toContain('data-label="show all 20 lines"');
    expect(seg[0]).not.toContain("clamp-toggle");
  });

  it("Truncated with forceExpand lifts every clamp", () => {
    const html = render("truncated", true);
    expect(html).not.toContain("1lh");
    expect(html).toContain('data-seg-id="3:1:tool-result"');
  });

  it("Everything opens every part with no height limit", () => {
    const html = render("everything");
    expect(html).toContain('data-seg-id="3:1:tool-input"');
    expect(html).toContain('data-seg-id="3:1:tool-result"');
    expect(html).not.toContain("max-height");
  });

  it("Inputs opens the input and leaves the output behind a sub-row", () => {
    const html = render("inputs");
    expect(html).toContain('data-seg-id="3:1:tool-input"');
    expect(html).not.toContain('data-seg-id="3:1:tool-result"');
    expect(html).toContain("output · 20 lines");
  });
});
