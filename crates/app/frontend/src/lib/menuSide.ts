/** Which way a dropdown opens, given where its trigger sits inside whatever clips it. */
export interface MenuSide {
  /** Draw the menu above the trigger rather than below it. */
  up: boolean;
  /** Cap the menu's height to this so it scrolls, when it fits neither side; `null` leaves it alone —
   *  capping a menu to its own measured height rounds it down a fraction and draws a needless scrollbar. */
  maxHeight: number | null;
}

/** Below the trigger while the menu fits there, else above while it fits there, else whichever side has
 *  more room. `bounds` is what would cut the menu off — a scrolling list, or the window. */
export function menuSide(
  trigger: { top: number; bottom: number },
  menuHeight: number,
  bounds: { top: number; bottom: number },
  gap = 4,
): MenuSide {
  const below = bounds.bottom - trigger.bottom - gap;
  const above = trigger.top - bounds.top - gap;
  if (menuHeight <= below) return { up: false, maxHeight: null };
  if (menuHeight <= above) return { up: true, maxHeight: null };
  return above > below ? { up: true, maxHeight: above } : { up: false, maxHeight: below };
}
