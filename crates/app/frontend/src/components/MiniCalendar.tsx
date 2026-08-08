import { useState } from "react";

/** Minimal, fully-themed date picker with an optional hh:mm field.
 *
 *  Exists because the native datetime-local control is unusable here:
 *  WKWebView paints the focused segment with the deep system accent and
 *  ignores styling of its shadow parts — three attempts (index.css,
 *  color-scheme, raw-served CSS bypassing lightningcss) changed nothing but
 *  the popover. Drawing our own ~40 cells costs less than fighting that.
 *
 *  Emits `yyyy-mm-dd` or `yyyy-mm-ddThh:mm` — exactly the query language's
 *  date forms, so the caller just inserts the value. */
export default function MiniCalendar({ onPick }: { onPick: (value: string) => void }) {
  const now = new Date();
  const [view, setView] = useState(() => new Date(now.getFullYear(), now.getMonth(), 1));
  const [time, setTime] = useState("");

  const pad = (n: number) => String(n).padStart(2, "0");
  const pick = (d: Date) => {
    const t = /^([01]\d|2[0-3]):([0-5]\d)$/.test(time.trim()) ? `T${time.trim()}` : "";
    onPick(`${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}${t}`);
  };

  // 6 fixed weeks starting on the Sunday at/before the 1st — a stable grid, so
  // the panel never changes height while browsing months.
  const first = new Date(view.getFullYear(), view.getMonth(), 1);
  const start = new Date(first);
  start.setDate(1 - first.getDay());
  const cells = Array.from({ length: 42 }, (_, i) => {
    const d = new Date(start);
    d.setDate(start.getDate() + i);
    return d;
  });
  const inMonth = (d: Date) => d.getMonth() === view.getMonth();
  const isToday = (d: Date) => d.toDateString() === now.toDateString();
  const monthLabel = view.toLocaleString([], { month: "short", year: "numeric" });

  return (
    <div className="mt-2 rounded border border-[var(--color-border)] bg-[var(--color-bg)] p-2 text-xs w-56">
      <div className="flex items-center justify-between mb-1">
        <span className="font-medium text-[var(--color-text)]">{monthLabel}</span>
        <span className="flex gap-1">
          <button
            onClick={() => setView(new Date(view.getFullYear(), view.getMonth() - 1, 1))}
            className="px-1.5 rounded text-[var(--color-text-dim)] hover:text-[var(--color-accent)] hover:bg-[var(--color-surface-hover)]"
            title="Previous month"
          >
            ‹
          </button>
          <button
            onClick={() => setView(new Date(now.getFullYear(), now.getMonth(), 1))}
            className="px-1.5 rounded text-[var(--color-text-dim)] hover:text-[var(--color-accent)] hover:bg-[var(--color-surface-hover)]"
            title="Current month"
          >
            •
          </button>
          <button
            onClick={() => setView(new Date(view.getFullYear(), view.getMonth() + 1, 1))}
            className="px-1.5 rounded text-[var(--color-text-dim)] hover:text-[var(--color-accent)] hover:bg-[var(--color-surface-hover)]"
            title="Next month"
          >
            ›
          </button>
        </span>
      </div>
      <div className="grid grid-cols-7 text-center text-[10px] text-[var(--color-text-dim)] mb-0.5">
        {["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"].map((d) => (
          <span key={d}>{d}</span>
        ))}
      </div>
      <div className="grid grid-cols-7">
        {cells.map((d) => (
          <button
            key={d.toISOString()}
            onClick={() => pick(d)}
            className={`py-0.5 rounded text-center hover:bg-[var(--color-accent)] hover:text-[var(--color-bg)] ${
              isToday(d)
                ? "text-[var(--color-accent)] font-semibold"
                : inMonth(d)
                  ? "text-[var(--color-text)]"
                  : "text-[var(--color-thinking)]"
            }`}
          >
            {d.getDate()}
          </button>
        ))}
      </div>
      <div className="flex items-center gap-1.5 mt-1.5">
        <input
          value={time}
          onChange={(e) => setTime(e.target.value)}
          placeholder="hh:mm"
          className="w-16 bg-[var(--color-surface)] text-[var(--color-text)] px-1.5 py-0.5 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
        />
        <span className="text-[var(--color-text-dim)]">optional — then click a day</span>
      </div>
    </div>
  );
}
