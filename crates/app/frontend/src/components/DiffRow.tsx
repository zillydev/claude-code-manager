import { diffLineText, type DiffLine } from "../lib/toolDiff";
import type { Token } from "../lib/syntax";
import { emphasize, type Range } from "../lib/wordDiff";

/** The stronger tint on the words of a replaced line that actually changed. */
export const EMPH_CLASS = {
  add: "rounded-[2px] bg-[color-mix(in_srgb,var(--color-assistant)_40%,transparent)]",
  del: "rounded-[2px] bg-[color-mix(in_srgb,var(--color-error)_40%,transparent)]",
};

/** One diff line, tinted across the full width, with its side's syntax tokens when there are any and the
 *  changed words of a replaced line emphasized. Emphasis only splits spans, so the text is unchanged. */
export default function DiffRow({
  line: l,
  tokens,
  emph,
  tail,
}: {
  line: DiffLine;
  tokens: Token[] | undefined;
  emph?: Range[];
  tail: string;
}) {
  const marked = l.kind === "add" || l.kind === "del";
  const base: Token[] =
    tokens && tokens.length > 0 ? tokens : [{ text: l.text, cls: marked ? undefined : "text-[var(--color-text-dim)]" }];
  const emphClass = l.kind === "add" ? EMPH_CLASS.add : l.kind === "del" ? EMPH_CLASS.del : "";
  return (
    <span
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
      {emphasize(base, emph ?? []).map((t, k) => (
        <span key={k} className={[t.cls, t.emph ? emphClass : ""].filter(Boolean).join(" ") || undefined}>
          {t.text}
        </span>
      ))}
      {tail}
    </span>
  );
}
