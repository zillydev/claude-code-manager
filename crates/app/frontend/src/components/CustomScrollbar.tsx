import { useCallback, useEffect, useRef, useState } from "react";

interface Props {
  /** The scrolling element this bar controls (its native scrollbar hidden). */
  scrollerRef: React.RefObject<HTMLElement | null>;
  /** Bump/replace to force a geometry re-measure (e.g. rendered turn count). */
  contentKey?: unknown;
}

/**
 * A DOM-drawn vertical scrollbar. Native macOS scrollbars in WKWebView hand
 * hover over to the overlay painter, whose thumb only draws mid-scroll — the
 * handle vanished exactly when the cursor reached for it, and the window's
 * rounded corner clipped its tail. Nothing native is involved here: the thumb
 * is a div (so it can't disappear), and the track is inset from the corner.
 *
 * Geometry re-measures on scroll, on scroller resize, on `contentKey` change,
 * and on a slow interval (content can grow without an event: images decoding,
 * tool bodies expanding). Hidden entirely when the content fits.
 */
export default function CustomScrollbar({ scrollerRef, contentKey }: Props) {
  const [geom, setGeom] = useState<{ top: number; height: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const trackRef = useRef<HTMLDivElement>(null);
  // Live drag state: pointer offset within the thumb, so a grab anywhere on
  // the thumb tracks 1:1 instead of snapping its centre to the cursor.
  const dragState = useRef<{ grabOffset: number } | null>(null);

  const measure = useCallback(() => {
    const el = scrollerRef.current;
    const track = trackRef.current;
    if (!el || !track) return;
    const { scrollTop, scrollHeight, clientHeight } = el;
    if (scrollHeight <= clientHeight + 1) {
      setGeom((g) => (g === null ? g : null));
      return;
    }
    const trackH = track.clientHeight;
    const height = Math.max(32, (clientHeight / scrollHeight) * trackH);
    const maxTop = trackH - height;
    const denom = scrollHeight - clientHeight;
    const top = denom <= 0 ? 0 : (scrollTop / denom) * maxTop;
    setGeom((g) => (g && Math.abs(g.top - top) < 0.5 && Math.abs(g.height - height) < 0.5 ? g : { top, height }));
  }, [scrollerRef]);

  useEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    measure();
    el.addEventListener("scroll", measure, { passive: true });
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    const iv = setInterval(measure, 1000); // growth without events (img decode)
    return () => {
      el.removeEventListener("scroll", measure);
      ro.disconnect();
      clearInterval(iv);
    };
  }, [scrollerRef, measure]);

  useEffect(() => {
    measure();
  }, [contentKey, measure]);

  const scrollToThumbTop = useCallback(
    (thumbTop: number) => {
      const el = scrollerRef.current;
      const track = trackRef.current;
      if (!el || !track || !geom) return;
      const maxTop = track.clientHeight - geom.height;
      const clamped = Math.max(0, Math.min(thumbTop, maxTop));
      el.scrollTop = (clamped / (maxTop || 1)) * (el.scrollHeight - el.clientHeight);
    },
    [scrollerRef, geom],
  );

  const onThumbPointerDown = (e: React.PointerEvent) => {
    if (!geom) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    const trackTop = trackRef.current!.getBoundingClientRect().top;
    dragState.current = { grabOffset: e.clientY - trackTop - geom.top };
    setDragging(true);
  };
  const onThumbPointerMove = (e: React.PointerEvent) => {
    if (!dragState.current) return;
    const trackTop = trackRef.current!.getBoundingClientRect().top;
    scrollToThumbTop(e.clientY - trackTop - dragState.current.grabOffset);
  };
  const onThumbPointerUp = (e: React.PointerEvent) => {
    e.currentTarget.releasePointerCapture(e.pointerId);
    dragState.current = null;
    setDragging(false);
  };

  // Click on the empty track: jump so the thumb centres on the click.
  const onTrackPointerDown = (e: React.PointerEvent) => {
    if (!geom || e.target !== trackRef.current) return;
    const trackTop = trackRef.current!.getBoundingClientRect().top;
    scrollToThumbTop(e.clientY - trackTop - geom.height / 2);
  };

  // The track must ALWAYS mount: measure() reads its clientHeight, so a
  // conditional render would deadlock (no track → no geom → no track).
  // visibility (not display) keeps it measurable while hidden.
  return (
    <div
      ref={trackRef}
      onPointerDown={onTrackPointerDown}
      // inset-y-2: keeps the thumb's travel clear of the window's rounded corner
      className="absolute right-0 inset-y-2 w-3 z-10"
      style={geom ? undefined : { visibility: "hidden", pointerEvents: "none" }}
    >
      {geom && (
      <div
        onPointerDown={onThumbPointerDown}
        onPointerMove={onThumbPointerMove}
        onPointerUp={onThumbPointerUp}
        className="absolute right-[2px] w-2 rounded-full transition-colors"
        style={{
          top: geom.top,
          height: geom.height,
          backgroundColor: dragging
            ? "var(--color-text-dim)"
            : "color-mix(in srgb, var(--color-text-dim) 40%, transparent)",
        }}
        onMouseEnter={(e) => {
          if (!dragging) (e.currentTarget as HTMLElement).style.backgroundColor = "color-mix(in srgb, var(--color-text-dim) 65%, transparent)";
        }}
        onMouseLeave={(e) => {
          if (!dragging) (e.currentTarget as HTMLElement).style.backgroundColor = "color-mix(in srgb, var(--color-text-dim) 40%, transparent)";
        }}
      />
      )}
    </div>
  );
}
