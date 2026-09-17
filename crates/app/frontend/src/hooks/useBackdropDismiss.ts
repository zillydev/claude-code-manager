import { useMemo, useRef } from "react";
import { backdropDismiss } from "../lib/backdropDismiss";

/** Handlers to spread on an overlay's backdrop: it closes on a click there, not at the end of a drag from inside. */
export function useBackdropDismiss(onClose: () => void) {
  const latest = useRef(onClose);
  latest.current = onClose;
  return useMemo(() => backdropDismiss(() => latest.current()), []);
}
