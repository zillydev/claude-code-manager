import { useCallback, useLayoutEffect, useRef, useState, type CSSProperties, type ReactElement } from "react";
import { boxStyle, clampLabel, type Height, type Pad } from "../lib/appearance";

type BoxRef = (el: HTMLElement | null) => void;
type RenderBox = (style: CSSProperties | undefined, ref?: BoxRef) => ReactElement;

/** Tailwind `py-2`, the padding of most body boxes. */
export const PY_2: Pad = { top: "0.5rem", bottom: "0.5rem" };

/** One collapsible body (tool input/output, a diff, a task result…) at its mode's height. `children` renders the
 *  box that carries the height; a clamp hides the overflow with CSS only, so find still sees the whole text. */
export default function BodyBox({
  height,
  scrollPx,
  pad,
  lines,
  modeSeq,
  forceFull,
  fade = "var(--color-tool-result-bg)",
  children,
}: {
  height: Height;
  scrollPx: number;
  pad?: Pad;
  /** Logical lines in the body, for "show all N lines". Only read under a clamp. */
  lines: number;
  /** The window's switch count (`useWindowView`); a lift made before the last switch is dropped. */
  modeSeq: number;
  /** Find landed inside: lift the clamp. */
  forceFull?: boolean;
  /** The colour the fade ends in: the box's background. */
  fade?: string;
  children: RenderBox;
}) {
  if (height.kind !== "clamp") return children(boxStyle(height, scrollPx));
  return (
    <Clamped clamp={height} pad={pad} lines={lines} modeSeq={modeSeq} forceFull={forceFull} fade={fade}>
      {children}
    </Clamped>
  );
}

type Geom = { over: boolean; left: number; bottom: number; width: number };

function Clamped({
  clamp,
  pad,
  lines,
  modeSeq,
  forceFull,
  fade,
  children,
}: {
  clamp: Height & { kind: "clamp" };
  pad?: Pad;
  lines: number;
  modeSeq: number;
  forceFull?: boolean;
  fade: string;
  children: RenderBox;
}) {
  const [liftedIn, setLiftedIn] = useState<number | null>(null);
  const userLifted = liftedIn === modeSeq;
  const lifted = userLifted || !!forceFull;
  const el = useRef<HTMLElement | null>(null);
  const setEl = useCallback((e: HTMLElement | null) => {
    el.current = e;
  }, []);
  // Guessed from the line count until measured, so static markup and the first paint already show the control.
  const [geom, setGeom] = useState<Geom>(() => ({ over: lines > clamp.lines, left: 0, bottom: 0, width: 0 }));

  useLayoutEffect(() => {
    const box = el.current;
    if (lifted || !box) return;
    const measure = () => {
      const next = {
        over: box.scrollHeight > box.clientHeight + 1,
        left: box.offsetLeft,
        bottom: box.offsetTop + box.offsetHeight,
        width: box.offsetWidth,
      };
      setGeom((g) =>
        g.over === next.over && g.left === next.left && g.bottom === next.bottom && g.width === next.width ? g : next,
      );
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(box);
    return () => ro.disconnect();
  }, [lifted, lines]);

  // "show less" folds the body away above the toggle; keep the toggle where it was on screen.
  const toggle = useRef<HTMLButtonElement | null>(null);
  const toggleTop = useRef<number | null>(null);
  useLayoutEffect(() => {
    const before = toggleTop.current;
    toggleTop.current = null;
    if (before === null || !toggle.current) return;
    const scroller = scrollParent(toggle.current);
    if (scroller) scroller.scrollTop += toggle.current.getBoundingClientRect().top - before;
  }, [liftedIn]);

  const label = clampLabel(lines, clamp.lines, userLifted);
  return (
    <>
      <div className="relative">
        {children(lifted ? undefined : boxStyle(clamp, 0, pad, geom.over), setEl)}
        {!lifted && geom.over && (
          <div
            aria-hidden
            className="pointer-events-none absolute rounded-b text-xs"
            style={{
              left: geom.left,
              width: geom.width || "100%",
              top: geom.bottom || undefined,
              bottom: geom.bottom ? undefined : 0,
              height: "1.5lh",
              transform: geom.bottom ? "translateY(-100%)" : undefined,
              background: `linear-gradient(to bottom, transparent, ${fade})`,
            }}
          />
        )}
      </div>
      {(userLifted || (!lifted && geom.over)) && (
        <button
          ref={toggle}
          type="button"
          data-label={label}
          aria-label={label}
          onClick={() => {
            if (userLifted) toggleTop.current = toggle.current?.getBoundingClientRect().top ?? null;
            setLiftedIn(userLifted ? null : modeSeq);
          }}
          className="clamp-toggle block text-[10px] px-1 mb-1 text-[var(--color-text-dim)] hover:text-[var(--color-accent)]"
          style={geom.left ? { marginLeft: geom.left } : undefined}
        />
      )}
    </>
  );
}

function scrollParent(el: HTMLElement): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    if (/(auto|scroll)/.test(getComputedStyle(p).overflowY)) return p;
  }
  return null;
}
