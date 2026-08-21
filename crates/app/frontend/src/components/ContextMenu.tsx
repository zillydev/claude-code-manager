import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { placeMenu } from "../lib/menuPlacement";

/** Open state for one surface's context menu: where it was summoned, plus
 *  whatever the caller needs to build its items (the row, the turn, …). */
export interface MenuState<T> {
  x: number;
  y: number;
  target: T;
}

/**
 * Per-surface context-menu state. `open(e, target)` records the pointer
 * position and suppresses the native menu for that click; `close()` dismisses.
 * Kept local to each surface — only one menu is ever open in practice, since
 * opening another dismisses the first via its outside-click handler.
 */
export function useContextMenu<T>() {
  const [menu, setMenu] = useState<MenuState<T> | null>(null);
  const open = useCallback((e: React.MouseEvent, target: T) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY, target });
  }, []);
  const close = useCallback(() => setMenu(null), []);
  return { menu, open, close };
}

/**
 * A menu drawn at the pointer. Fixed-position so it escapes every scroller,
 * measured after mount and then placed (flipped near edges — see placeMenu).
 * Dismisses on Esc, any click, scroll, or the window losing focus: a context
 * menu that outlives its context is worse than no menu.
 */
export default function ContextMenu({
  x,
  y,
  onClose,
  children,
}: {
  x: number;
  y: number;
  onClose: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  // Rendered invisibly for one frame so the real size is known before placing;
  // otherwise a menu near an edge visibly jumps after mount.
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos(
      placeMenu(
        x,
        y,
        { width: r.width, height: r.height },
        { width: window.innerWidth, height: window.innerHeight },
      ),
    );
  }, [x, y]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    // Close on a mousedown OUTSIDE the menu. Testing containment (rather than
    // relying on stopPropagation in the menu's own handler) is what makes the
    // items clickable at all: this listener is on the capture phase, so it
    // runs BEFORE the event reaches the button — closing unconditionally
    // unmounted the menu between mousedown and mouseup, and no item ever
    // received a click.
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("blur", onClose);
    // A scrolling pane would leave the menu stranded over unrelated content.
    window.addEventListener("scroll", onClose, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("scroll", onClose, true);
    };
  }, [onClose]);

  return (
    <div
      ref={ref}
      role="menu"
      onContextMenu={(e) => e.preventDefault()}
      style={{
        position: "fixed",
        left: pos?.left ?? x,
        top: pos?.top ?? y,
        visibility: pos ? "visible" : "hidden",
      }}
      className="z-50 min-w-44 rounded border border-[var(--color-border)] bg-[var(--color-surface)] py-1 shadow-lg"
    >
      {children}
    </div>
  );
}
