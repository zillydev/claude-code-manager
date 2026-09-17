import { Activity, X } from "lucide-react";
import { usePersistentState } from "../hooks/usePersistentState";
import { usePreciseStatusChoice } from "../hooks/usePreciseStatusChoice";
import { showsPreciseStatusNotice } from "../lib/preciseStatus";

/** Foot of the sidebar: says once that the app turned precise status on by adding a plugin to Claude Code. */
export default function PreciseStatusNote({ onOpenSettings }: { onOpenSettings: () => void }) {
  const choice = usePreciseStatusChoice();
  const [dismissed, setDismissed] = usePersistentState("cm:preciseStatusNoticeDismissed", false);
  if (!showsPreciseStatusNotice(choice, dismissed)) return null;
  return (
    <div className="shrink-0 flex items-start gap-1.5 px-3 py-2 border-t border-[var(--color-border)] text-xs">
      <Activity size={13} strokeWidth={2} className="shrink-0 mt-px text-[var(--color-accent)]" />
      <p className="flex-1 min-w-0 text-[var(--color-text)]">
        Precise session status is on — ccm added its status plugin to Claude Code.{" "}
        <button
          type="button"
          onClick={onOpenSettings}
          className="text-[var(--color-accent)] underline underline-offset-2 cursor-pointer hover:opacity-80"
        >
          Settings
        </button>
      </p>
      <button
        onClick={() => setDismissed(true)}
        title="Dismiss"
        aria-label="Dismiss precise status notice"
        className="shrink-0 flex items-center p-0.5 text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
      >
        <X size={12} strokeWidth={2} />
      </button>
    </div>
  );
}
