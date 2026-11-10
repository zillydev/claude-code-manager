/** Tallest canvas we draw, in device px: comfortably inside WebKit's canvas limits at transcript widths. */
const MAX_PX = 16384;
const PAD = 16;

/** Render scale for a message `cssHeight` tall; null when it would need to drop below half scale. */
export function captureScale(cssHeight: number, dpr: number): number | null {
  const scale = Math.min(dpr, 2, MAX_PX / cssHeight);
  return scale < 0.5 ? null : scale;
}

/** Leaves out the hover actions and bookmark flag, marked `data-capture-skip`. */
export function keepInCapture(node: Node): boolean {
  return (node as Partial<HTMLElement>).dataset?.captureSkip === undefined;
}

/** A turn as it is displayed, framed in the transcript background, as PNG bytes. */
export async function captureTurn(node: HTMLElement): Promise<Uint8Array> {
  const width = node.offsetWidth + 2 * PAD;
  const height = node.offsetHeight + 2 * PAD;
  const scale = captureScale(height, window.devicePixelRatio || 1);
  if (scale === null) throw new Error("Message too tall to capture");
  const { domToBlob } = await import("modern-screenshot");
  const blob = await domToBlob(node, {
    width,
    height,
    scale,
    type: "image/png",
    backgroundColor: getComputedStyle(document.documentElement).getPropertyValue("--color-bg").trim() || null,
    filter: keepInCapture,
    style: { padding: `${PAD}px`, margin: "0" },
  });
  return new Uint8Array(await blob.arrayBuffer());
}
