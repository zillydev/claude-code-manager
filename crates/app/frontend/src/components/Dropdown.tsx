import { useState, useRef, useEffect, useLayoutEffect, useCallback, type ReactNode } from "react";
import { menuSide, type MenuSide } from "../lib/menuSide";

/** What would cut the menu off: the nearest scrolling ancestor (the sessions list), else the window. */
function clipperOf(el: HTMLElement | null): { top: number; bottom: number } {
  for (let p = el?.parentElement; p; p = p.parentElement) {
    if (getComputedStyle(p).overflowY !== "visible") {
      const r = p.getBoundingClientRect();
      return { top: r.top, bottom: r.bottom };
    }
  }
  return { top: 0, bottom: window.innerHeight };
}

/** Shared styling for a button rendered as a Dropdown menu item. */
export const menuItemClass =
  "block w-full text-left text-[11px] px-3 py-1.5 hover:bg-[var(--color-surface-hover)] disabled:opacity-50";

/** A click-to-open menu: owns open state, closes on Esc / click-outside. The
 *  trigger is a button; `children(close)` renders the items and may call `close`.
 *  Used for the per-turn and per-session action menus. */
export default function Dropdown({
  trigger,
  title,
  align = "left",
  fullWidth = false,
  disabled = false,
  triggerClassName = "",
  onOpenChange,
  children,
}: {
  trigger: ReactNode;
  title?: string;
  align?: "left" | "right";
  /** Stretch the menu across the nearest positioned ancestor instead of
   *  anchoring it to the trigger — for menus inside a narrow container (the
   *  sidebar), where a trigger-anchored menu of fixed width spills outside it. */
  fullWidth?: boolean;
  disabled?: boolean;
  triggerClassName?: string;
  /** Observe open/close — e.g. to reset per-open state like "show archived". */
  onOpenChange?: (open: boolean) => void;
  children: (close: () => void) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  // Which way this one opens, measured while it is on screen but before it is painted.
  const [side, setSide] = useState<MenuSide | null>(null);
  useLayoutEffect(() => {
    if (!open || !triggerRef.current || !menuRef.current) {
      setSide(null);
      return;
    }
    const trigger = triggerRef.current.getBoundingClientRect();
    setSide(menuSide(trigger, menuRef.current.scrollHeight, clipperOf(triggerRef.current)));
  }, [open]);
  const close = useCallback(() => setOpen(false), []);
  const onOpenChangeRef = useRef(onOpenChange);
  onOpenChangeRef.current = onOpenChange;
  useEffect(() => {
    onOpenChangeRef.current?.(open);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open, close]);

  return (
    // `static` under fullWidth so the menu resolves against the container
    // (the filters row), not this wrapper.
    <div className={fullWidth ? "static" : "relative"} ref={ref}>
      <button
        ref={triggerRef}
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          if (!disabled) setOpen((o) => !o);
        }}
        disabled={disabled}
        title={title}
        aria-haspopup="menu"
        aria-expanded={open}
        className={triggerClassName}
      >
        {trigger}
      </button>
      {open && (
        <div
          ref={menuRef}
          role="menu"
          style={side?.maxHeight != null ? { maxHeight: side.maxHeight, overflowY: "auto" } : undefined}
          className={`absolute z-30 rounded border border-[var(--color-border)] bg-[var(--color-surface)] py-1 shadow-lg ${
            side?.up ? "bottom-full mb-1" : "top-full mt-1"
          } ${fullWidth ? "left-0 right-0" : `${align === "right" ? "right-0" : "left-0"} min-w-32`}`}
        >
          {children(close)}
        </div>
      )}
    </div>
  );
}

/** A destructive menu item that requires a second click to confirm. The confirm
 *  state lives here, so it resets whenever the menu closes (this unmounts). */
export function ConfirmMenuItem({
  label,
  confirmLabel,
  onConfirm,
  close,
}: {
  label: string;
  confirmLabel: string;
  onConfirm: () => void;
  close: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  return confirming ? (
    <button
      role="menuitem"
      className={`${menuItemClass} text-[var(--color-error)] font-medium`}
      onClick={(e) => {
        e.stopPropagation();
        close();
        onConfirm();
      }}
    >
      {confirmLabel}
    </button>
  ) : (
    <button
      role="menuitem"
      className={`${menuItemClass} text-[var(--color-error)]`}
      onClick={(e) => {
        e.stopPropagation();
        setConfirming(true);
      }}
    >
      {label}
    </button>
  );
}
