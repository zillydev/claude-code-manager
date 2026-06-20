/** Scroll metrics of a scrollable element — the subset `isNearBottom` needs. */
export interface ScrollMetrics {
  scrollHeight: number;
  scrollTop: number;
  clientHeight: number;
}

/**
 * Whether a scroll container is at (or within `threshold` px of) its bottom.
 * Used for tail-follow: only auto-scroll on new content when the user is
 * already pinned to the bottom, so an arriving live-tail message never yanks
 * them down while they're reading further up. A fully-visible (non-scrollable)
 * container counts as at-bottom.
 */
export function isNearBottom(m: ScrollMetrics, threshold = 80): boolean {
  return m.scrollHeight - m.scrollTop - m.clientHeight <= threshold;
}
