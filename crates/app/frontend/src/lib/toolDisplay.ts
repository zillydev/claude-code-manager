/** Display helpers for tool calls — pure, so they're testable without DOM. */

/** `mcp__deployer-prod-mcp__fetch_app_logs` → `deployer-prod-mcp · fetch_app_logs`.
 *  MCP tool names are machine-namespaced; the transcript should read like a
 *  tool list, not a wire format. Non-MCP names pass through untouched. */
export function prettyToolName(name: string): string {
  if (!name.startsWith("mcp__")) return name;
  const parts = name.slice(5).split("__");
  return parts.length >= 2 ? `${parts[0]} · ${parts.slice(1).join("__")}` : name;
}

/** Pretty-print a tool result that is actually JSON (the common MCP shape:
 *  a single-line JSON blob). Returns the original text when it isn't JSON,
 *  is already multi-line (someone formatted it), or is huge (avoid churning
 *  megabyte payloads for no reader benefit). */
export function prettyResultText(text: string): string {
  const t = text.trim();
  if (t.length > 200_000 || text.includes("\n")) return text;
  if (!(t.startsWith("{") || t.startsWith("["))) return text;
  try {
    return JSON.stringify(JSON.parse(t), null, 2);
  } catch {
    return text;
  }
}
