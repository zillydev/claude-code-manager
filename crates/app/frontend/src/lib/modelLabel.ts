import type { ModelUsage } from "../types";

export function shortModel(model: string): string {
  return model.replace("claude-", "");
}

/** An assistant turn's label: "opus-5-5 · high", or the model alone when the line has no effort. */
export function turnModelLabel(model: string | null, effort: string | null): string | null {
  const parts = [model && shortModel(model), effort].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : null;
}

/** The effort breakdown after a model's count in the session header, or null when no line recorded one. */
export function effortSummary(m: ModelUsage): string | null {
  const { efforts } = m;
  if (efforts.length === 0) return null;
  if (efforts.length === 1 && efforts[0].turn_count === m.turn_count) return efforts[0].effort;
  return efforts.map((e) => `${e.effort} ×${e.turn_count}`).join(" · ");
}
