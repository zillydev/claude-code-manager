/** The files a shell command changed, as the transcript draws them under its result. Pure, so the
 *  renderer and the find matcher derive the same text. */
import type { FileChange, FileChanges, Hunk } from "../types";
import { diffLineText, type DiffLine } from "./toolDiff";

/** One hunk: where it starts, its lines, and the two texts they index into for highlighting. */
export interface HunkView {
  /** "⋯ line N", or null for a hunk at the top of the file. */
  gap: string | null;
  lines: DiffLine[];
  old: string;
  new: string;
}

export interface FileView {
  verb: "Created" | "Updated";
  path: string;
  added: number;
  removed: number;
  hunks: HunkView[];
}

export function filesLabel(n: number): string {
  return `${n} file${n === 1 ? "" : "s"}`;
}

export function changeCounts(changes: FileChanges): { files: number; added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const f of changes.files) {
    const v = fileView(f);
    added += v.added;
    removed += v.removed;
  }
  return { files: changes.files.length + changes.more, added, removed };
}

function hunkView(h: Hunk): HunkView {
  const oldSide: string[] = [];
  const newSide: string[] = [];
  const lines: DiffLine[] = [];
  for (const raw of h.lines) {
    const mark = raw[0];
    const text = raw.slice(1);
    if (mark === "\\") continue; // "\ No newline at end of file"
    if (mark === "+") {
      lines.push({ kind: "add", text, side: "new", index: newSide.length });
      newSide.push(text);
    } else if (mark === "-") {
      lines.push({ kind: "del", text, side: "old", index: oldSide.length });
      oldSide.push(text);
    } else {
      lines.push({ kind: "ctx", text, side: "new", index: newSide.length });
      oldSide.push(text);
      newSide.push(text);
    }
  }
  return {
    gap: h.new_start > 1 ? `⋯ line ${h.new_start}` : null,
    lines,
    old: oldSide.join("\n"),
    new: newSide.join("\n"),
  };
}

export function fileView(file: FileChange): FileView {
  const hunks = file.hunks.map(hunkView);
  const count = (kind: DiffLine["kind"]) =>
    hunks.reduce((n, h) => n + h.lines.filter((l) => l.kind === kind).length, 0);
  return {
    verb: file.created ? "Created" : "Updated",
    path: file.path,
    added: count("add"),
    removed: count("del"),
    hunks,
  };
}

/** A file's heading line: "Updated /repo/a.md +1 −0", or "Created /repo/b.md +12". */
export function fileHeader(view: FileView): string {
  const counts = view.verb === "Created" ? `+${view.added}` : `+${view.added} −${view.removed}`;
  return `${view.verb} ${view.path} ${counts}`;
}

/** "…and 2 more files" when Claude Code left files out, else null. */
export function moreFilesText(changes: FileChanges): string | null {
  const n = changes.more;
  return n > 0 ? `…and ${n} more file${n === 1 ? "" : "s"}` : null;
}

/** Every line the transcript draws for `changes`, joined — what find searches. */
export function fileChangesText(changes: FileChanges): string {
  const rows: string[] = [];
  for (const f of changes.files) {
    const view = fileView(f);
    rows.push(fileHeader(view));
    for (const h of view.hunks) {
      if (h.gap) rows.push(h.gap);
      rows.push(...h.lines.map(diffLineText));
    }
  }
  const more = moreFilesText(changes);
  if (more) rows.push(more);
  return rows.join("\n");
}
