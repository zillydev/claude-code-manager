import { Fragment, useState } from "react";
import { Check, CircleArrowUp, Copy, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { usePersistentState } from "../hooks/usePersistentState";
import { useUpdateAvailable } from "../hooks/useUpdateAvailable";
import { noteVersion, UPGRADE_COMMAND } from "../lib/updateNotice";
import { releaseUrl } from "../lib/repo";

/** The version, opening its release notes in the browser. */
export function ReleaseLink({ version }: { version: string }) {
  const url = releaseUrl(version);
  return (
    <button
      type="button"
      onClick={() => openUrl(url).catch((e) => console.error("openUrl failed:", e))}
      title={url}
      className="text-xs text-[var(--color-accent)] underline underline-offset-2 cursor-pointer hover:opacity-80"
    >
      v{version}
    </button>
  );
}

/** The upgrade command in a code box with a copy button. */
export function UpgradeCommand() {
  const [copied, setCopied] = useState(false);
  const copy = () =>
    navigator.clipboard.writeText(UPGRADE_COMMAND).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1200);
      },
      () => {},
    );
  return (
    <div className="flex items-start gap-1.5 rounded border border-[var(--color-border)] bg-[var(--color-bg)] pl-2 pr-1 py-1">
      <code className="flex-1 min-w-0 font-mono text-[0.6875rem] leading-4 text-[var(--color-text)] select-text">
        {/* Each word unbreakable, so a narrow sidebar wraps between words rather than at `--`. */}
        {UPGRADE_COMMAND.split(" ").map((word, i) => (
          <Fragment key={i}>
            {i > 0 && " "}
            <span className="whitespace-nowrap">{word}</span>
          </Fragment>
        ))}
      </code>
      <button
        type="button"
        onClick={copy}
        title={copied ? "Copied" : "Copy command"}
        aria-label="Copy upgrade command"
        className="shrink-0 flex items-center p-0.5 text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
      >
        {copied ? <Check size={12} strokeWidth={2} /> : <Copy size={12} strokeWidth={2} />}
      </button>
    </div>
  );
}

/** Foot of the sidebar: a newer release is out. Dismissing hides that version in every window. */
export default function UpdateNote() {
  const latest = useUpdateAvailable();
  const [dismissed, setDismissed] = usePersistentState<string | null>("cm:dismissedUpdate", null);
  const version = noteVersion(latest, dismissed);
  if (!version) return null;
  return (
    <div className="shrink-0 flex flex-col gap-1.5 px-3 py-2 border-t border-[var(--color-border)]">
      <div className="flex items-center gap-1.5">
        <CircleArrowUp size={13} strokeWidth={2} className="shrink-0 text-[var(--color-accent)]" />
        <span className="text-xs text-[var(--color-text)]">Update available</span>
        <ReleaseLink version={version} />
        <button
          onClick={() => setDismissed(version)}
          title="Dismiss"
          aria-label="Dismiss update notice"
          className="ml-auto shrink-0 flex items-center p-0.5 text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
        >
          <X size={12} strokeWidth={2} />
        </button>
      </div>
      <UpgradeCommand />
    </div>
  );
}
