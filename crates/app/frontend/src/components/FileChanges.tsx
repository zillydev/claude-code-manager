import { useMemo } from "react";
import type { FileChanges } from "../types";
import { changeCounts, filesLabel, fileView, moreFilesText, type FileView } from "../lib/fileChanges";
import { highlightToLines, languageForPath } from "../lib/syntax";
import { wordEmphasis } from "../lib/wordDiff";
import DiffRow from "./DiffRow";

/** "2 files +3 −1" on the collapsed header of a command that changed files. */
export function ChangesBadge({ changes }: { changes: FileChanges }) {
  const c = changeCounts(changes);
  return (
    <span className="ml-auto pl-3 shrink-0 text-xs tabular-nums text-[var(--color-text-dim)]">
      {filesLabel(c.files)} <span className="text-[var(--color-assistant)]">+{c.added}</span>{" "}
      <span className="text-[var(--color-error)]">−{c.removed}</span>
    </span>
  );
}

/** The files a command changed, each as a heading over its diff.
 *
 *  Same rule as the other tool bodies: the element's textContent equals `fileChangesText(changes)`, so every
 *  drawn row ends in a newline except the last, keeping find offsets aligned. */
export function FileChangesBody({ changes, segId }: { changes: FileChanges; segId: string }) {
  const views = useMemo(() => changes.files.map(fileView), [changes]);
  const more = moreFilesText(changes);
  const rowsIn = (v: FileView) => 1 + v.hunks.reduce((n, h) => n + (h.gap ? 1 : 0) + h.lines.length, 0);
  const total = views.reduce((n, v) => n + rowsIn(v), 0) + (more ? 1 : 0);
  let first = 0;
  return (
    <div data-seg-id={segId} className="mt-1 space-y-1">
      {views.map((v, i) => {
        const start = first;
        first += rowsIn(v);
        return <FileDiff key={i} view={v} firstRow={start} totalRows={total} />;
      })}
      {more && <div className="px-1 text-xs italic text-[var(--color-text-dim)]">{more}</div>}
    </div>
  );
}

function FileDiff({ view, firstRow, totalRows }: { view: FileView; firstRow: number; totalRows: number }) {
  // Hunks aren't contiguous, so each one's sides are highlighted on their own.
  const tokens = useMemo(() => {
    const lang = languageForPath(view.path);
    if (!lang) return null;
    return view.hunks.map((h) => ({ old: highlightToLines(h.old, lang), new: highlightToLines(h.new, lang) }));
  }, [view]);
  const emph = useMemo(() => view.hunks.map((h) => wordEmphasis(h.lines)), [view]);

  let row = firstRow;
  const tail = () => (++row === totalRows ? "" : "\n");
  const header = tail();
  return (
    <div>
      <div className="px-1 text-xs text-[var(--color-text-dim)] break-all">
        <span className="font-medium text-[var(--color-text)]">{view.verb}</span> {view.path}{" "}
        <span className="text-[var(--color-assistant)]">+{view.added}</span>
        {view.verb === "Updated" && (
          <>
            {" "}
            <span className="text-[var(--color-error)]">−{view.removed}</span>
          </>
        )}
        {header}
      </div>
      {view.hunks.length > 0 && (
        <pre
          className="px-3 py-2 rounded text-xs whitespace-pre-wrap break-words overflow-hidden border-l-2 border-l-[var(--color-tool)] bg-[var(--color-tool-result-bg)] text-[var(--color-text)]"
          style={{ maxHeight: "300px", overflowY: "auto" }}
        >
          {view.hunks.map((h, hi) => (
            <span key={hi}>
              {h.gap && (
                <span className="block italic text-[var(--color-text-dim)]">
                  {h.gap}
                  {tail()}
                </span>
              )}
              {h.lines.map((l, li) => (
                <DiffRow
                  key={li}
                  line={l}
                  tokens={tokens && l.side ? tokens[hi][l.side][l.index ?? -1] : undefined}
                  emph={emph[hi][li]}
                  tail={tail()}
                />
              ))}
            </span>
          ))}
        </pre>
      )}
    </div>
  );
}
