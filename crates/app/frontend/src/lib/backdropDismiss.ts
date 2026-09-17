interface BackdropEvent {
  target: unknown;
  currentTarget: unknown;
}

/** Backdrop handlers that close an overlay only for a click pressed and released on the backdrop itself. A drag
 *  that starts in the dialog (selecting text) and ends outside fires `click` on the backdrop too — the common
 *  ancestor of both ends — and must not close it. */
export function backdropDismiss(onClose: () => void) {
  let pressed = false;
  let released = false;
  return {
    onMouseDown: (e: BackdropEvent) => {
      pressed = e.target === e.currentTarget;
    },
    onMouseUp: (e: BackdropEvent) => {
      released = e.target === e.currentTarget;
    },
    onClick: (e: BackdropEvent) => {
      const onBackdrop = pressed && released && e.target === e.currentTarget;
      pressed = released = false;
      if (onBackdrop) onClose();
    },
  };
}
