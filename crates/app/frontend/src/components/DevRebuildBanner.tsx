import { X } from "lucide-react";
import Spinner from "./Spinner";
import { useDevRebuild } from "../hooks/useDevRebuild";

/** Floating status for the dev-only Rebuild & Relaunch; renders nothing when idle. */
export default function DevRebuildBanner() {
  const { progress, dismiss, retry } = useDevRebuild();
  if (!progress) return null;

  return (
    <div className={`fixed bottom-3 right-3 z-50 ${progress.phase === "failed" ? "w-[min(640px,calc(100vw-24px))]" : ""} rounded border border-[var(--color-border)] bg-[var(--color-surface)] shadow-lg text-sm`}>
      {progress.phase === "failed" ? (
        <>
          <div className="flex items-center gap-3 px-3 py-2 border-b border-[var(--color-border)]">
            <span className="font-medium text-[var(--color-error)]">Build failed</span>
            <span className="text-xs text-[var(--color-text-dim)]">still running the previous build</span>
            <button
              onClick={retry}
              className="ml-auto px-2 py-0.5 rounded bg-[var(--color-accent)] text-white text-xs"
            >
              Retry
            </button>
            <button
              onClick={dismiss}
              title="Dismiss"
              className="text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
            >
              <X size={14} />
            </button>
          </div>
          <pre className="max-h-80 overflow-auto px-3 py-2 text-xs font-mono whitespace-pre text-[var(--color-text)]">
            {progress.output}
          </pre>
        </>
      ) : (
        <div className="flex items-center gap-2 px-3 py-2 text-[var(--color-text-dim)]">
          <Spinner />
          {progress.phase === "building" ? "Rebuilding…" : "Relaunching…"}
        </div>
      )}
    </div>
  );
}
