import { useEffect, useState } from "react";
import type { RenderedImage } from "../types";

/** An image carried by a message or tool result, rendered from its base64
 *  data (no file I/O — the bytes are already in the session payload).
 *  Thumbnail in the flow; click opens a Discord-style lightbox: dimmed
 *  backdrop, image centred at up to ~90% of the viewport. Backdrop click or
 *  Esc closes. `fixed` positioning escapes the tool-result scroller, and the
 *  house-modal z-50/backdrop treatment matches Settings/LineageTree. */
export default function InlineImage({ image, alt }: { image: RenderedImage; alt: string }) {
  const [open, setOpen] = useState(false);
  const src = `data:${image.media_type};base64,${image.data}`;

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation(); // don't also close a find bar / modal underneath
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open]);

  return (
    <>
      <img
        src={src}
        alt={alt}
        onClick={() => setOpen(true)}
        title="Click to view"
        className="block max-h-64 max-w-full rounded border border-[var(--color-border)] cursor-zoom-in"
      />
      {open && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 cursor-zoom-out"
          onClick={() => setOpen(false)}
        >
          <img
            src={src}
            alt={alt}
            onClick={(e) => e.stopPropagation()}
            className="max-w-[90vw] max-h-[90vh] rounded shadow-2xl cursor-default"
          />
        </div>
      )}
    </>
  );
}

/** The images strip under a message / inside a tool result; renders nothing
 *  for the common no-images case. */
export function ImageStrip({ images, label }: { images?: RenderedImage[]; label: string }) {
  if (!images || images.length === 0) return null;
  return (
    <div className="flex flex-col gap-2 mt-2">
      {images.map((img, i) => (
        <InlineImage key={i} image={img} alt={`${label} ${i + 1}`} />
      ))}
    </div>
  );
}
