/** The Manager ↔ Terminal switch. Lives in the session header (right cluster on
 *  Manager, slim header on Terminal) rather than its own bar, to save a row.
 *  The Terminal button carries a live-terminal dot and a hover-✕ to close it. */
export default function TabToggle({
  activeTab,
  onSelect,
  terminalLive = false,
  onClose,
}: {
  activeTab: "manager" | "terminal";
  onSelect: (t: "manager" | "terminal") => void;
  /** Show a green dot on the Terminal button (this window has a live PTY for the
   *  current session). */
  terminalLive?: boolean;
  /** Close (kill) the current session's terminal; enables the hover-✕. */
  onClose?: () => void;
}) {
  const btn = (active: boolean) =>
    "px-3 py-1 text-sm rounded " +
    (active
      ? "bg-[var(--color-accent)] text-white"
      : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]");
  const showClose = terminalLive && !!onClose;
  return (
    <div className="flex gap-1">
      <button className={btn(activeTab === "manager")} onClick={() => onSelect("manager")}>
        Manager
      </button>
      {/* group: the ✕ reveals on hover over the Terminal button. */}
      <div className="group relative flex items-center">
        <button
          className={btn(activeTab === "terminal") + (showClose ? " pr-6" : "")}
          onClick={() => onSelect("terminal")}
        >
          <span className="inline-flex items-center gap-1.5">
            {terminalLive && (
              <span
                className="inline-block w-1.5 h-1.5 rounded-full bg-green-500"
                title="A claude terminal is running for this session"
              />
            )}
            Terminal
          </span>
        </button>
        {showClose && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onClose!();
            }}
            title="Close terminal session (stops claude)"
            className={
              "absolute right-1.5 opacity-0 group-hover:opacity-100 text-xs leading-none px-0.5 " +
              (activeTab === "terminal"
                ? "text-white/80 hover:text-white"
                : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]")
            }
          >
            ✕
          </button>
        )}
      </div>
    </div>
  );
}
