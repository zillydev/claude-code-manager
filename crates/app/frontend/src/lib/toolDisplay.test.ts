import { describe, it, expect } from "vitest";
import { prettyToolName, prettyResultText } from "./toolDisplay";

describe("prettyToolName", () => {
  it("splits MCP wire names into server · tool", () => {
    expect(prettyToolName("mcp__deployer-prod-mcp__fetch_app_logs")).toBe(
      "deployer-prod-mcp · fetch_app_logs",
    );
  });
  it("keeps double-underscore tool names whole after the server part", () => {
    expect(prettyToolName("mcp__srv__a__b")).toBe("srv · a__b");
  });
  it("passes non-MCP names through", () => {
    expect(prettyToolName("Bash")).toBe("Bash");
  });
});

describe("prettyResultText", () => {
  it("pretty-prints single-line JSON objects and arrays", () => {
    expect(prettyResultText('{"a":1,"b":[2,3]}')).toBe('{\n  "a": 1,\n  "b": [\n    2,\n    3\n  ]\n}');
    expect(prettyResultText("[1,2]")).toBe("[\n  1,\n  2\n]");
  });
  it("leaves non-JSON and already-formatted text alone", () => {
    expect(prettyResultText("plain output")).toBe("plain output");
    expect(prettyResultText('{\n  "a": 1\n}')).toBe('{\n  "a": 1\n}');
    expect(prettyResultText("{broken")).toBe("{broken");
  });
});
