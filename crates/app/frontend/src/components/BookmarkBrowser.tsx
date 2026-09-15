import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SessionSummary, ToolPreview } from "../types";
import ToolPreviewLine from "./ToolPreviewLine";
import { useSessionMeta } from "../hooks/useSessionMeta";
import { sessionTitle } from "../lib/sessionMeta";

interface Preview {
  uuid: string;
  snippet: string;
  timestamp: number | null;
  tool: ToolPreview | null;
}

/** All bookmarks across every discovered session, grouped by session, newest
 *  session first. Click → open that session scrolled-to + flashing the message
 *  (the lineage-tree plumbing). Sessions absent from discovery (trashed) simply
 *  don't render; their bookmarks return with them on restore. */
export default function BookmarkBrowser({
  sessions,
  onOpen,
}: {
  sessions: SessionSummary[];
  onOpen: (s: SessionSummary, messageUuid: string) => void;
}) {
  const { meta } = useSessionMeta();
  // uuid → preview per session id; fetched once per open for the listed groups.
  const [previews, setPreviews] = useState<Record<string, Record<string, Preview>>>({});

  const groups = sessions
    .filter((s) => (meta.sessions[s.id]?.bookmarks?.length ?? 0) > 0)
    .sort((a, b) => b.mtime.secs_since_epoch - a.mtime.secs_since_epoch);

  useEffect(() => {
    let alive = true;
    for (const s of groups) {
      if (previews[s.id]) continue;
      const uuids = meta.sessions[s.id]?.bookmarks ?? [];
      invoke<Preview[]>("message_previews", { filePath: s.file_path, uuids })
        .then((rows) => {
          if (!alive) return;
          setPreviews((prev) => ({
            ...prev,
            [s.id]: Object.fromEntries(rows.map((r) => [r.uuid, r])),
          }));
        })
        .catch(() => {});
    }
    return () => {
      alive = false;
    };
    // groups is derived; keying on the ids string avoids a fetch loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [groups.map((g) => g.id).join(","), meta]);

  const fmtTime = (epoch: number | null) =>
    epoch === null
      ? ""
      : new Date(epoch * 1000).toLocaleString([], {
          month: "short",
          day: "numeric",
          hour: "2-digit",
          minute: "2-digit",
        });

  return (
    <div className="h-full min-h-0 overflow-y-auto p-2 space-y-3">
        {groups.length === 0 && (
          <div className="text-xs text-[var(--color-text-dim)]">
            No bookmarks yet — use ⋯ → Bookmark on any message.
          </div>
        )}

        {groups.map((s) => {
          const uuids = meta.sessions[s.id]?.bookmarks ?? [];
          const p = previews[s.id] ?? {};
          return (
            <div key={s.id} className="space-y-1">
              <div className="flex items-baseline gap-2 text-xs">
                <span className="font-medium text-[var(--color-text)] truncate">
                  {sessionTitle(meta, s.id, s.title)}
                </span>
                <span className="text-[var(--color-text-dim)] shrink-0">
                  {s.project_label} · {uuids.length}
                </span>
              </div>
              {uuids.map((u) => (
                <button
                  key={u}
                  onClick={() => onOpen(s, u)}
                  className="w-full text-left text-xs px-2 py-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] hover:border-[var(--color-accent)] flex items-baseline gap-2"
                >
                  <span className="text-[var(--color-accent)] shrink-0">⚑</span>
                  <span className="truncate text-[var(--color-text)]">
                    {p[u]?.tool ? <ToolPreviewLine tool={p[u].tool} /> : (p[u]?.snippet ?? "(message not found)")}
                  </span>
                  <span className="ml-auto shrink-0 text-[var(--color-text-dim)]">
                    {fmtTime(p[u]?.timestamp ?? null)}
                  </span>
                </button>
              ))}
            </div>
          );
        })}
    </div>
  );
}
