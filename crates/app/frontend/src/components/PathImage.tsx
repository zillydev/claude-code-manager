import { createContext, useContext, useEffect, useMemo, useState, type ComponentProps, type ReactNode } from "react";
import { defaultUrlTransform, type ExtraProps, type UrlTransform } from "react-markdown";
import { invoke } from "@tauri-apps/api/core";
import { Check, Copy, ImageOff } from "lucide-react";
import InlineImage from "./InlineImage";
import { codeImagePaths, makeImageLoader, resolveImageSrc, savedImagePaths, type ImageLoader } from "../lib/imagePaths";
import type { RenderedImage, RenderedTurn } from "../types";

interface ImagePathCtx {
  saved: Map<string, RenderedImage>;
  cwd: string | null;
  /** Null means nothing is read from disk. */
  load: ImageLoader | null;
}

export const ImagePathContext = createContext<ImagePathCtx>({ saved: new Map(), cwd: null, load: null });

/** The provider value for one open session; the read cache lives as long as the session stays open. */
export function useImagePathCtx(sessionId: string, turns: RenderedTurn[], cwd: string | null): ImagePathCtx {
  const saved = useMemo(() => savedImagePaths(turns), [turns]);
  const load = useMemo(() => makeImageLoader((cmd, args) => invoke<RenderedImage>(cmd, args)), [sessionId]);
  return useMemo(() => ({ saved, cwd, load }), [saved, cwd, load]);
}

type Status = { kind: "ok"; image: RenderedImage } | { kind: "loading" } | { kind: "missing"; reason: string };

function useLocalImage(path: string): Status {
  const { saved, load } = useContext(ImagePathContext);
  const hit = saved.get(path);
  const [read, setRead] = useState<Status>(load ? { kind: "loading" } : { kind: "missing", reason: "not_found" });
  useEffect(() => {
    if (hit || !load) return;
    let live = true;
    setRead({ kind: "loading" });
    load(path).then(
      (image) => live && setRead({ kind: "ok", image }),
      (err) => live && setRead({ kind: "missing", reason: String(err) }),
    );
    return () => {
      live = false;
    };
  }, [path, hit, load]);
  return hit ? { kind: "ok", image: hit } : read;
}

function CopyPath({ path }: { path: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      onClick={() =>
        navigator.clipboard.writeText(path).then(
          () => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          },
          () => {},
        )
      }
      className="shrink-0 inline-flex h-[1lh] items-center text-[var(--color-text-dim)] hover:text-[var(--color-accent)] transition-colors"
      title="Copy path"
    >
      {copied ? <Check size={11} /> : <Copy size={11} />}
    </button>
  );
}

const chipClass = "flex items-start gap-1.5 mt-1 font-mono text-[11px] text-[var(--color-text-dim)] select-text break-all";

/** A markdown image: local paths preview from the session or disk, with the path to copy under it. */
function PathImage({ src, alt }: ComponentProps<"img"> & ExtraProps) {
  const { cwd } = useContext(ImagePathContext);
  const path = resolveImageSrc(src, cwd);
  if (!path) {
    if (!src || /^(https?:|data:|\/\/)/i.test(src)) return <img src={src} alt={alt} />;
    return (
      <span className={`not-prose ${chipClass}`}>
        <span>{src}</span>
        <CopyPath path={src} />
      </span>
    );
  }
  return <LocalImage path={path} alt={alt ?? ""} />;
}

function LocalImage({ path, alt }: { path: string; alt: string }) {
  const status = useLocalImage(path);
  return (
    <span className="not-prose block my-2">
      {status.kind === "ok" && <InlineImage image={status.image} alt={alt || path} />}
      {status.kind === "loading" && (
        <span className="block h-16 w-32 rounded border border-[var(--color-border)] bg-[var(--color-surface)]" />
      )}
      <span className={chipClass}>
        {status.kind === "missing" && (
          <>
            <ImageOff size={12} className="shrink-0 h-[1lh]" />
            {/* Drawn by CSS: text arriving after the read would shift in-session find's offsets. */}
            <span
              className="shrink-0 before:content-[attr(data-label)]"
              data-label={`${status.reason === "too_large" ? "image too large" : "image not found"} ·`}
            />
          </>
        )}
        {alt && <span className="shrink-0 font-sans">{alt} ·</span>}
        <span>{path}</span>
        <CopyPath path={path} />
      </span>
    </span>
  );
}

/** A small preview of a backticked path; nothing at all unless it resolves. */
function CodePathThumb({ path }: { path: string }) {
  const status = useLocalImage(path);
  if (status.kind !== "ok") return null;
  return (
    <span className="not-prose block w-fit mt-1 mb-2">
      <InlineImage image={status.image} alt={path} className="max-h-32" />
    </span>
  );
}

function CodePathThumbs({ node }: ExtraProps) {
  return codeImagePaths(node).map((p) => <CodePathThumb key={p} path={p} />);
}

function Paragraph({ node, children, ...rest }: ComponentProps<"p"> & ExtraProps): ReactNode {
  return (
    <>
      <p {...rest}>{children}</p>
      <CodePathThumbs node={node} />
    </>
  );
}

function ListItem({ node, children, ...rest }: ComponentProps<"li"> & ExtraProps) {
  return (
    <li {...rest}>
      {children}
      <CodePathThumbs node={node} />
    </li>
  );
}

export const MD_IMAGE_COMPONENTS = { img: PathImage, p: Paragraph, li: ListItem };

/** react-markdown drops `file:` URLs as unsafe; an image's is safe since only `read_image_file` resolves it. */
export const mdUrlTransform: UrlTransform = (url, key, node) =>
  key === "src" && node.tagName === "img" && url.startsWith("file://") ? url : defaultUrlTransform(url);
