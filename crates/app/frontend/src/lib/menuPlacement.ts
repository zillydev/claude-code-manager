/** Where a context menu should be drawn, given the click point and the space
 *  available. Pure, so the flipping rules are testable without a DOM. */
export interface Placement {
  left: number;
  top: number;
}

/**
 * Place a menu at the pointer, flipped back inside the viewport when it would
 * overflow. Flips rather than clamps: a menu pinned to the edge would sit under
 * the cursor and swallow the next click. Falls back to clamping only when the
 * menu is taller than the viewport itself.
 *
 * @param x,y            pointer position (client coords)
 * @param menu           the menu's measured size
 * @param view           viewport size
 * @param pad            keep this far from every edge
 */
export function placeMenu(
  x: number,
  y: number,
  menu: { width: number; height: number },
  view: { width: number; height: number },
  pad = 6,
): Placement {
  // Prefer down-right of the cursor; flip to the other side when it won't fit.
  let left = x + menu.width + pad > view.width ? x - menu.width : x;
  let top = y + menu.height + pad > view.height ? y - menu.height : y;
  // A menu taller/wider than the space either way: clamp into view.
  left = Math.max(pad, Math.min(left, view.width - menu.width - pad));
  top = Math.max(pad, Math.min(top, view.height - menu.height - pad));
  return { left: Math.max(pad, left), top: Math.max(pad, top) };
}
