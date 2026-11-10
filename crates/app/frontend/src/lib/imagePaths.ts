/** Local image paths the agent writes in its replies, and where their bytes come from. */
import type { RenderedImage, RenderedTurn } from "../types";

const IMAGE_EXT = /\.(png|jpe?g|gif|webp|bmp|svg|avif|heic)$/i;
// claude-in-chrome's wording next to the screenshot it returns; the file itself is a temp that is soon deleted.
const SAVED_RE = /(?:^|\n)\s*Screenshot saved to:\s*(\S+)/g;

function decode(p: string): string {
  try {
    return decodeURIComponent(p);
  } catch {
    return p;
  }
}

/** The local path a markdown image `src` names (absolute or `~/…`), or null
 *  for remote/data sources, non-images, and relative paths with no cwd. */
export function resolveImageSrc(src: string | undefined, cwd: string | null): string | null {
  if (!src) return null;
  let p = src.trim();
  if (p.startsWith("file://")) p = p.slice("file://".length);
  else if (/^[a-z][a-z0-9+.-]*:/i.test(p) || p.startsWith("//")) return null;
  p = decode(p);
  if (!IMAGE_EXT.test(p)) return null;
  if (p.startsWith("/") || p.startsWith("~/")) return p;
  if (!cwd) return null;
  return `${cwd.replace(/\/+$/, "")}/${p.replace(/^\.\//, "")}`;
}

/** An inline-code span that is an absolute or `~/` image path, trimmed. */
export function codeImagePath(text: string): string | null {
  const p = text.trim();
  if (p.includes("\n") || p.startsWith("//")) return null;
  if (!(p.startsWith("/") || p.startsWith("~/"))) return null;
  return IMAGE_EXT.test(p) ? p : null;
}

/** The hast shape react-markdown hands its components. */
interface HastNode {
  type: string;
  tagName?: string;
  value?: string;
  children?: unknown[];
}

const BLOCK_TAGS = new Set(["p", "ul", "ol", "li", "pre", "blockquote", "table"]);

/** Image paths in a paragraph's or list item's own inline code; nested blocks render their own. */
export function codeImagePaths(node: unknown): string[] {
  const out: string[] = [];
  const walk = (n: HastNode, root: boolean) => {
    if (n.type !== "element" && n.type !== "root") return;
    if (!root && n.tagName && BLOCK_TAGS.has(n.tagName)) return;
    if (n.tagName === "code") {
      const text = (n.children ?? []).map((c) => (c as HastNode).value ?? "").join("");
      const p = codeImagePath(text);
      if (p && !out.includes(p)) out.push(p);
      return;
    }
    for (const c of n.children ?? []) walk(c as HastNode, false);
  };
  if (node) walk(node as HastNode, true);
  return out;
}

/** Screenshots a tool returned, keyed by the path it said it saved them to.
 *  Paired by index, and only when the counts agree. */
export function savedImagePaths(turns: RenderedTurn[]): Map<string, RenderedImage> {
  const out = new Map<string, RenderedImage>();
  for (const turn of turns) {
    if (turn.type !== "assistant") continue;
    for (const b of turn.blocks) {
      if (b.type !== "tool_call" || !b.result?.images?.length) continue;
      const paths = [...b.result.text.matchAll(SAVED_RE)].map((m) => m[1]);
      if (paths.length !== b.result.images.length) continue;
      paths.forEach((p, i) => out.set(p, b.result!.images![i]));
    }
  }
  return out;
}

export type ImageLoader = (path: string) => Promise<RenderedImage>;

/** Reads through the `read_image_file` command, once per path. */
export function makeImageLoader(
  invoke: (cmd: string, args: Record<string, unknown>) => Promise<RenderedImage>,
): ImageLoader {
  const cache = new Map<string, Promise<RenderedImage>>();
  return (path) => {
    let p = cache.get(path);
    if (!p) {
      p = invoke("read_image_file", { path });
      cache.set(path, p);
    }
    return p;
  };
}
