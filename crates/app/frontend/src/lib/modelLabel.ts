import type { ModelUsage } from "../types";

export function shortModel(model: string): string {
  return model.replace("claude-", "");
}

/** The effort breakdown after a model's count in the session header, or null when no line recorded one. */
export function effortSummary(m: ModelUsage): string | null {
  const { efforts } = m;
  if (efforts.length === 0) return null;
  if (efforts.length === 1 && efforts[0].turn_count === m.turn_count) return efforts[0].effort;
  return efforts.map((e) => `${e.effort} ×${e.turn_count}`).join(" · ");
}
