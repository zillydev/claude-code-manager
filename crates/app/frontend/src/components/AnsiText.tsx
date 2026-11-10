import { memo, useMemo, type CSSProperties } from "react";
import { ansiSegments, type AnsiStyle } from "../lib/ansi";

function css(s: AnsiStyle): CSSProperties {
  const out: CSSProperties = {};
  const fg = s.inverse ? (s.bg ?? "var(--color-tool-result-bg)") : s.fg;
  const bg = s.inverse ? s.fg : s.bg;
  if (fg) out.color = fg;
  if (bg) out.backgroundColor = bg;
  if (s.bold) out.fontWeight = 600;
  if (s.dim) out.opacity = 0.65;
  if (s.italic) out.fontStyle = "italic";
  const lines = [s.underline && "underline", s.strike && "line-through"].filter(Boolean);
  if (lines.length) out.textDecoration = lines.join(" ");
  return out;
}

/** Terminal output with its ANSI styling. Adds no characters, so its
 *  textContent equals `ansiText(text)` and find offsets stay aligned. */
export const AnsiText = memo(function AnsiText({ text }: { text: string }) {
  const segs = useMemo(() => ansiSegments(text), [text]);
  return (
    <>
      {segs.map((seg, i) => {
        if (!seg.style) return seg.text;
        const span = (
          <span key={i} style={css(seg.style)}>
            {seg.text}
          </span>
        );
        // Inverse with the default foreground paints the inherited text colour
        // as background, which only an outer element still sees as currentColor.
        return seg.style.inverse && !seg.style.fg ? (
          <span key={i} style={{ backgroundColor: "currentColor" }}>
            {span}
          </span>
        ) : (
          span
        );
      })}
    </>
  );
});
