import { useState, useRef, useEffect, useCallback, type ReactNode } from "react";

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
          role="menu"
          className={`absolute top-full mt-1 z-30 rounded border border-[var(--color-border)] bg-[var(--color-surface)] py-1 shadow-lg ${
            fullWidth ? "left-0 right-0" : `${align === "right" ? "right-0" : "left-0"} min-w-32`
          }`}
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
