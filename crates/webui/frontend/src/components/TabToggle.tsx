/** The Manager ↔ Terminal switch. Lives in the session header (right cluster on
 *  Manager, slim header on Terminal) rather than its own bar, to save a row. */
export default function TabToggle({
  activeTab,
  onSelect,
}: {
  activeTab: "manager" | "terminal";
  onSelect: (t: "manager" | "terminal") => void;
}) {
  return (
    <div className="flex gap-1">
      {(["manager", "terminal"] as const).map((t) => (
        <button
          key={t}
          onClick={() => onSelect(t)}
          className={
            "px-3 py-1 text-sm rounded " +
            (activeTab === t
              ? "bg-[var(--color-accent)] text-white"
              : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]")
          }
        >
          {t === "manager" ? "Manager" : "Terminal"}
        </button>
      ))}
    </div>
  );
}
