import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SessionSummary } from "../types";
import { buildLineageTree, type LineageNode } from "../lib/lineageTree";

export default function LineageTree({
  sessions,
  centerId,
  onOpen,
  onClose,
}: {
  sessions: SessionSummary[];
  centerId: string;
  onOpen: (s: SessionSummary, scrollToMessage?: string) => void;
  onClose: () => void;
}) {
  const root = useMemo(() => buildLineageTree(sessions, centerId), [sessions, centerId]);
  const [snippets, setSnippets] = useState<Record<string, string>>({});

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    if (!root) return;
    const refs: { file_path: string; message_uuid: string }[] = [];
    const walk = (n: LineageNode) => {
      if (n.branchMessageUuid && n.branchParentFilePath) {
        refs.push({ file_path: n.branchParentFilePath, message_uuid: n.branchMessageUuid });
      }
      n.children.forEach(walk);
    };
    walk(root);
    if (refs.length === 0) return;
    invoke<{ message_uuid: string; snippet: string }[]>("resolve_branch_messages", { refs })
      .then((rows) => setSnippets(Object.fromEntries(rows.map((r) => [r.message_uuid, r.snippet]))))
      .catch(() => {});
  }, [root]);

  if (!root) return null;

  const renderNode = (node: LineageNode, depth: number, parent?: LineageNode) => {
    const isCurrent = node.session.id === centerId;
    const snippet = node.branchMessageUuid ? snippets[node.branchMessageUuid] : undefined;
    const label =
      node.edge === "root"
        ? "◀ origin"
        : node.edge === "forked"
          ? snippet
            ? `⑂ forked at “${snippet}”`
            : "⑂ forked here"
          : `⧉ copied${node.copiedOverlap != null ? ` · ${node.copiedOverlap} shared` : ""}`;
    // The branch label links to the parent session, scrolled to the branch message.
    const canJumpToBranch = !!(parent && node.branchMessageUuid);
    return (
      <div key={node.session.id} style={{ paddingLeft: depth * 18 }}>
        {node.edge === "root" ? (
          <div className="mt-1.5 text-[10px] text-[var(--color-text-dim)]">{label}</div>
        ) : (
          <button
            onClick={() => canJumpToBranch && onOpen(parent!.session, node.branchMessageUuid!)}
            disabled={!canJumpToBranch}
            title={canJumpToBranch ? "Open the parent session at this point" : undefined}
            className="mt-1.5 block text-left text-[10px] text-[var(--color-text-dim)] hover:text-[var(--color-accent)] disabled:cursor-default"
          >
            {label}
          </button>
        )}
        <button
          disabled={isCurrent}
          onClick={() => !isCurrent && onOpen(node.session)}
          className={`block w-full text-left text-sm rounded px-2 py-1 ${
            isCurrent
              ? "bg-[var(--color-surface-hover)] text-[var(--color-accent)] font-semibold cursor-default"
              : "text-[var(--color-text)] hover:bg-[var(--color-surface-hover)]"
          }`}
        >
          <span className="mr-1.5 text-xs text-[var(--color-accent)]">{node.session.project_label}</span>
          <span className="truncate">{node.session.title}</span>
          {isCurrent && <span className="ml-2 text-xs">● you</span>}
        </button>
        {node.children.map((c) => renderNode(c, depth + 1, node))}
      </div>
    );
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-full max-w-2xl max-h-[80vh] overflow-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-2 flex items-center justify-between">
          <span className="text-sm font-medium text-[var(--color-text)]">Session lineage</span>
          <button
            onClick={onClose}
            className="text-sm text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
          >
            ×
          </button>
        </div>
        {renderNode(root, 0)}
      </div>
    </div>
  );
}
