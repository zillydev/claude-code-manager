import { useState } from "react";
import type { RenderedImage } from "../types";

/** An image carried by a message or tool result, rendered from its base64
 *  data (no file I/O — the bytes are already in the session payload).
 *  Thumbnail-sized by default; click toggles full width. */
export default function InlineImage({ image, alt }: { image: RenderedImage; alt: string }) {
  const [expanded, setExpanded] = useState(false);
  return (
    <img
      src={`data:${image.media_type};base64,${image.data}`}
      alt={alt}
      onClick={() => setExpanded(!expanded)}
      title={expanded ? "Click to shrink" : "Click to enlarge"}
      className={`block rounded border border-[var(--color-border)] cursor-zoom-in ${
        expanded ? "max-w-full cursor-zoom-out" : "max-h-64 max-w-full"
      }`}
    />
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
