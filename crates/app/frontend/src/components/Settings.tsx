import { useEffect, useMemo, type ReactNode } from "react";
import { useConfig } from "../hooks/useConfig";
import { THEME_LIST, UI_FONTS, MONO_FONTS } from "../lib/config";
import { useSessions } from "../hooks/useSessions";
import { matchProjectGroups, projectGroups } from "../lib/filterMatch";
import Dropdown from "./Dropdown";
import FilterMenu, { type MenuItem } from "./FilterMenu";

/** Settings overlay: theme, typography, and behavior defaults. Every control
 *  applies live via setConfig (which persists + broadcasts to other windows). */
export default function Settings({ onClose }: { onClose: () => void }) {
  const { config, setConfig } = useConfig();
  const t = config.typography;
  const ui = config.ui;
  // The same session list the sidebar groups — served from the shared cache,
  // so opening Settings costs no extra scan.
  const { sessions } = useSessions();
  const groups = useMemo(() => projectGroups(sessions), [sessions]);
  const defaultProjectLabel = useMemo(() => {
    const v = ui.defaultProject;
    if (v.startsWith("name:")) return v.slice(5);
    if (v.startsWith("dir:")) return v.slice(4).replace(/^\/Users\/[^/]+/, "~");
    return "None";
  }, [ui.defaultProject]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const stepBtn =
    "w-6 h-6 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]";

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md max-h-[85vh] overflow-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl space-y-4"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-[var(--color-text)]">Settings</span>
          <button
            onClick={onClose}
            className="text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
          >
            ×
          </button>
        </div>

        <Section title="Theme">
          <div className="flex gap-2">
            {THEME_LIST.map((th) => (
              <button
                key={th.key}
                onClick={() => setConfig({ theme: th.key })}
                className={`flex-1 text-xs px-3 py-2 rounded border ${
                  config.theme === th.key
                    ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                    : "border-[var(--color-border)] text-[var(--color-text-dim)] hover:border-[var(--color-accent)]"
                }`}
              >
                {th.label}
              </button>
            ))}
          </div>
        </Section>

        <Section title="Typography">
          <Row label="Font size">
            <div className="flex items-center gap-2">
              <button
                onClick={() => setConfig({ typography: { fontSize: Math.max(12, t.fontSize - 1) } })}
                className={stepBtn}
              >
                −
              </button>
              <span className="text-xs w-10 text-center text-[var(--color-text)]">
                {t.fontSize}px
              </span>
              <button
                onClick={() => setConfig({ typography: { fontSize: Math.min(18, t.fontSize + 1) } })}
                className={stepBtn}
              >
                +
              </button>
            </div>
          </Row>
          <Row label="Density">
            <Toggle2
              value={t.density}
              options={[
                ["comfortable", "Comfortable"],
                ["compact", "Compact"],
              ]}
              onChange={(v) => setConfig({ typography: { density: v as "comfortable" | "compact" } })}
            />
          </Row>
          <Row label="UI font">
            <FontSelect
              value={t.uiFont}
              presets={UI_FONTS}
              onChange={(v) => setConfig({ typography: { uiFont: v } })}
            />
          </Row>
          <Row label="Mono font">
            <FontSelect
              value={t.monoFont}
              presets={MONO_FONTS}
              onChange={(v) => setConfig({ typography: { monoFont: v } })}
            />
          </Row>
        </Section>

        <Section title="Behavior">
          <Row label="Default tab on launch">
            <Toggle2
              value={ui.defaultTab}
              options={[
                ["manager", "Manager"],
                ["terminal", "Terminal"],
              ]}
              onChange={(v) => setConfig({ ui: { defaultTab: v as "manager" | "terminal" } })}
            />
          </Row>
          <Row label="Default project on launch">
            <Dropdown
              align="right"
              trigger={defaultProjectLabel}
              title="Sidebar starts filtered to this project"
              triggerClassName={`text-xs px-1.5 py-1 rounded border outline-none max-w-48 truncate ${
                ui.defaultProject
                  ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                  : "border-[var(--color-border)] text-[var(--color-text)] bg-[var(--color-bg)]"
              }`}
            >
              {(close) => (
                <FilterMenu
                  close={close}
                  search={{ placeholder: "Filter projects…" }}
                  empty="No projects match"
                  items={(q) => {
                    const out: MenuItem[] = [];
                    if (!q.trim()) {
                      out.push({
                        key: "__none__",
                        node: "None",
                        selected: ui.defaultProject === "",
                        onSelect: () => {
                          setConfig({ ui: { defaultProject: "" } });
                          close();
                        },
                      });
                    }
                    for (const g of matchProjectGroups(groups, q)) {
                      const pick = (value: string) => () => {
                        setConfig({ ui: { defaultProject: value } });
                        close();
                      };
                      if (g.cwds.length > 1) {
                        g.cwds.forEach((c, i) => {
                          out.push({
                            key: `dir:${c}`,
                            node: c.replace(/^\/Users\/[^/]+/, "~"),
                            title: c,
                            section: i === 0 ? g.name : undefined,
                            selected: ui.defaultProject === `dir:${c}`,
                            onSelect: pick(`dir:${c}`),
                          });
                        });
                      } else {
                        out.push({
                          key: `name:${g.name}`,
                          node: g.name,
                          title: g.cwds[0],
                          selected: ui.defaultProject === `name:${g.name}`,
                          onSelect: pick(`name:${g.name}`),
                        });
                      }
                    }
                    return out;
                  }}
                />
              )}
            </Dropdown>
          </Row>
          <CheckRow
            label="Compact transcript"
            checked={ui.compact}
            onChange={(v) => setConfig({ ui: { compact: v } })}
          />
          <CheckRow
            label="Show details panel"
            checked={ui.detailsOpen}
            onChange={(v) => setConfig({ ui: { detailsOpen: v } })}
          />
          <CheckRow
            label="Start with sidebar collapsed"
            checked={ui.sidebarCollapsed}
            onChange={(v) => setConfig({ ui: { sidebarCollapsed: v } })}
          />
        </Section>

        <Section title="Integrations">
          <CheckRow
            label="Publish session status"
            checked={config.integrations.publishStatus}
            onChange={(v) => setConfig({ integrations: { publishStatus: v } })}
          />
          <div className="text-[10px] text-[var(--color-text-dim)] leading-relaxed">
            Writes live session state (running / unread / idle) to{" "}
            <code>session-status.json</code> in the config folder, for other tools to read.
            Removed when switched off.
          </div>
        </Section>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="space-y-2 border-t border-[var(--color-border)] pt-3">
      <div className="text-[10px] uppercase tracking-wide text-[var(--color-text-dim)]">{title}</div>
      {children}
    </div>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <span className="text-xs text-[var(--color-text)]">{label}</span>
      {children}
    </div>
  );
}

function Toggle2({
  value,
  options,
  onChange,
}: {
  value: string;
  options: [string, string][];
  onChange: (v: string) => void;
}) {
  return (
    <div className="flex rounded border border-[var(--color-border)] overflow-hidden text-xs">
      {options.map(([val, label]) => (
        <button
          key={val}
          onClick={() => onChange(val)}
          className={`px-2 py-1 ${
            value === val
              ? "bg-[var(--color-accent)] text-white"
              : "text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
          }`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

function CheckRow({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-center justify-between text-xs cursor-pointer text-[var(--color-text)]">
      <span>{label}</span>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
    </label>
  );
}

/** A preset dropdown plus a "Custom…" free-text field. The stored value is a
 *  preset key, or an arbitrary font-family string when custom. */
function FontSelect({
  value,
  presets,
  onChange,
}: {
  value: string;
  presets: { key: string; label: string }[];
  onChange: (v: string) => void;
}) {
  const isPreset = presets.some((p) => p.key === value);
  return (
    <div className="flex flex-col gap-1 items-end">
      <select
        value={isPreset ? value : "__custom__"}
        onChange={(e) => onChange(e.target.value === "__custom__" ? "" : e.target.value)}
        className="text-xs bg-[var(--color-bg)] text-[var(--color-text)] border border-[var(--color-border)] rounded px-1.5 py-1"
      >
        {presets.map((p) => (
          <option key={p.key} value={p.key}>
            {p.label}
          </option>
        ))}
        <option value="__custom__">Custom…</option>
      </select>
      {!isPreset && (
        <input
          type="text"
          autoFocus
          value={value}
          placeholder="Font family name"
          onChange={(e) => onChange(e.target.value)}
          className="text-xs bg-[var(--color-bg)] text-[var(--color-text)] border border-[var(--color-border)] rounded px-1.5 py-1 w-44 outline-none focus:border-[var(--color-accent)]"
        />
      )}
    </div>
  );
}
