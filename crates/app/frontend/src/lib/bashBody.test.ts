import { describe, it, expect } from "vitest";
import { bashLines, bashText } from "./bashBody";

const text = (input: Record<string, unknown>) => bashText(bashLines(input)!);

describe("bashLines", () => {
  it("puts the description above the command as a comment", () => {
    expect(text({ command: "ls -la", description: "List files" })).toBe("# List files\nls -la");
  });

  it("keeps a multi-line script as real lines", () => {
    const script = 'for f in a b; do\n  echo "$f"\ndone';
    expect(text({ command: script })).toBe(script);
    expect(bashLines({ command: script })!.filter((l) => l.kind === "cmd")).toHaveLength(3);
  });

  it("renders timeout and background as readable notes", () => {
    expect(text({ command: "x", timeout: 300000 })).toBe("# timeout 5m\nx");
    expect(text({ command: "x", timeout: 30000 })).toBe("# timeout 30s\nx");
    expect(text({ command: "x", timeout: 1500 })).toBe("# timeout 1500ms\nx");
    expect(text({ command: "x", run_in_background: true })).toBe("# runs in the background\nx");
  });

  it("orders notes description → background → timeout, then the script", () => {
    expect(
      text({ command: "x\ny", description: "d", run_in_background: true, timeout: 60000 }).split("\n"),
    ).toEqual(["# d", "# runs in the background", "# timeout 1m", "x", "y"]);
  });

  it("returns null without a usable command, so JSON is shown instead", () => {
    expect(bashLines({ description: "no command" })).toBeNull();
    expect(bashLines({ command: "" })).toBeNull();
    expect(bashLines({ command: 42 })).toBeNull();
  });
});
