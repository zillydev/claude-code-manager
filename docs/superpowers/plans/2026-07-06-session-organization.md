# Session Organization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Claude-manager-local session organization — global colored tags, a local alias shown alongside claude's auto-name, per-session notes, and a sidebar tag filter — stored in a separate `session-meta.json`, never touching claude's `.jsonl`.

**Architecture:** A new backend store (`session_meta.rs`, mirroring `config.rs`: atomic JSON load/save + `get/set_session_meta` commands + a `session-meta-changed` broadcast) and a frontend `SessionMetaProvider` context (mirroring `useConfig`). The sidebar *joins* `SessionSummary × meta.sessions[id]` for display and filtering — `SessionSummary` and the backend session list are untouched.

**Tech Stack:** Rust (serde, tauri 2), React 19 + TypeScript, vitest.

## Global Constraints

- NEVER write metadata into claude's session `.jsonl` files (copy-on-edit rule; the old `custom-title` rename was removed for exactly this).
- Do NOT launch `make webui` / `cargo tauri dev` — the user runs the dev app.
- Merges are user-directed; commit per task on `feat/session-organization` only.
- Every task ends green: `cargo test` (all crates), and in `crates/webui/frontend`: `npx tsc --noEmit`, `npx vitest run`, `npx vite build`.
- Spec: `docs/superpowers/specs/2026-07-06-session-organization-design.md`.

---

### Task 1: Backend store — `session_meta.rs` + commands

**Files:**
- Create: `crates/webui/src/session_meta.rs`
- Modify: `crates/webui/src/config.rs` (make `config_dir()` `pub(crate)`)
- Modify: `crates/webui/src/commands.rs` (two commands, after `set_config`)
- Modify: `crates/webui/src/lib.rs` (`mod session_meta;` + register commands)

**Interfaces:**
- Consumes: `config::config_dir() -> PathBuf` (currently private `fn config_dir()` in `config.rs`).
- Produces: `session_meta::{SessionMeta, Tag, SessionEntry, load, save}`; Tauri commands `get_session_meta() -> SessionMeta`, `set_session_meta(meta) -> Result<(), String>` (broadcasts `session-meta-changed` with the meta as payload). JSON wire shape: `{ version, tags: [{name, color}], sessions: { "<id>": { alias?, tags?, note? } } }`.

- [ ] **Step 1: Make `config_dir` crate-visible**

In `crates/webui/src/config.rs` change:

```rust
fn config_dir() -> PathBuf {
```

to:

```rust
pub(crate) fn config_dir() -> PathBuf {
```

- [ ] **Step 2: Write `session_meta.rs` with tests**

Create `crates/webui/src/session_meta.rs`:

```rust
//! Claude-manager-local session organization: global colored tags, per-session
//! alias/tags/note. Stored as JSON at `<config-dir>/session-meta.json` —
//! NEVER inside claude's session `.jsonl` (a live `claude` writes those files,
//! and the old in-file `custom-title` rename desynced for exactly that reason).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    #[serde(default = "default_version")]
    pub version: u32,
    /// Global tag registry: `name` is the identity, `color` a hex like "#539bf5".
    #[serde(default)]
    pub tags: Vec<Tag>,
    /// Per-session metadata keyed by session id. BTreeMap for stable file diffs.
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

fn default_version() -> u32 {
    1
}

impl Default for SessionMeta {
    fn default() -> Self {
        SessionMeta { version: default_version(), tags: Vec::new(), sessions: BTreeMap::new() }
    }
}

pub fn meta_path() -> PathBuf {
    crate::config::config_dir().join("session-meta.json")
}

/// Read + parse. Missing or malformed → defaults (a malformed file is left
/// intact so the user can fix it). Partial JSON merges over field defaults.
fn load_from(path: &Path) -> SessionMeta {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => SessionMeta::default(),
    }
}

fn save_to(path: &Path, meta: &SessionMeta) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(meta)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Load the store. A genuinely absent file is created with defaults.
pub fn load() -> SessionMeta {
    let path = meta_path();
    if !path.exists() {
        let meta = SessionMeta::default();
        let _ = save_to(&path, &meta);
        return meta;
    }
    load_from(&path)
}

pub fn save(meta: &SessionMeta) -> std::io::Result<()> {
    save_to(&meta_path(), meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cm-meta-test-{name}")).join("session-meta.json")
    }

    #[test]
    fn roundtrip_save_load() {
        let path = tmp("roundtrip");
        let mut meta = SessionMeta::default();
        meta.tags.push(Tag { name: "wip".into(), color: "#e5534b".into() });
        meta.sessions.insert(
            "abc".into(),
            SessionEntry {
                alias: Some("auth refactor".into()),
                tags: vec!["wip".into()],
                note: Some("blocked on X".into()),
            },
        );
        save_to(&path, &meta).unwrap();
        assert_eq!(load_from(&path), meta);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_file_is_default_empty() {
        let path = tmp("missing-xyz");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(load_from(&path), SessionMeta::default());
    }

    #[test]
    fn malformed_file_is_default() {
        let path = tmp("malformed");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ nope").unwrap();
        assert_eq!(load_from(&path), SessionMeta::default());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn partial_json_merges_defaults() {
        let path = tmp("partial");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Only a tag registry; version + sessions default. Entry fields default too.
        std::fs::write(
            &path,
            r#"{ "tags": [{"name":"wip","color":"#e5534b"}], "sessions": { "abc": {} } }"#,
        )
        .unwrap();
        let meta = load_from(&path);
        assert_eq!(meta.version, 1);
        assert_eq!(meta.tags.len(), 1);
        let e = &meta.sessions["abc"];
        assert_eq!(e.alias, None);
        assert!(e.tags.is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn atomic_save_leaves_no_tmp() {
        let path = tmp("atomic");
        save_to(&path, &SessionMeta::default()).unwrap();
        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn empty_entry_fields_are_skipped_in_json() {
        let json = serde_json::to_string(&SessionEntry {
            alias: None,
            tags: vec![],
            note: None,
        })
        .unwrap();
        assert_eq!(json, "{}");
    }
}
```

- [ ] **Step 3: Run the tests — expect FAIL to compile (module not registered)**

Run: `cargo test -p claude-manager-webui session_meta`
Expected: compile error — `session_meta` not in the crate. (If it compiles standalone, fine; the module registration comes next either way.)

- [ ] **Step 4: Register the module and commands**

In `crates/webui/src/lib.rs` add the module (alphabetical with the others):

```rust
mod commands;
mod config;
mod session_meta;
mod shared;
mod terminal;
mod window;
```

and register the two commands next to the config ones in the `invoke_handler` list:

```rust
            commands::get_config,
            commands::set_config,
            commands::get_session_meta,
            commands::set_session_meta,
```

In `crates/webui/src/commands.rs`, directly after the `set_config` command:

```rust
/// Load the session-organization metadata (creating the file on first run).
#[tauri::command]
pub fn get_session_meta() -> crate::session_meta::SessionMeta {
    crate::session_meta::load()
}

/// Persist session-organization metadata and broadcast it to every window.
#[tauri::command]
pub fn set_session_meta(
    meta: crate::session_meta::SessionMeta,
    app: AppHandle,
) -> Result<(), String> {
    crate::session_meta::save(&meta).map_err(|e| e.to_string())?;
    let _ = app.emit("session-meta-changed", &meta);
    Ok(())
}
```

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p claude-manager-webui 2>&1 | grep -E "test result|error"`
Expected: `session_meta` tests pass; webui total becomes 18 (12 + 6); no errors.

Run: `cargo test 2>&1 | grep -E "test result|error"`
Expected: all crates green (107 core + 3 tui + 18 webui).

- [ ] **Step 6: Commit**

```bash
git add crates/webui/src/session_meta.rs crates/webui/src/config.rs crates/webui/src/commands.rs crates/webui/src/lib.rs
git commit -m "feat(webui): session-meta store — tags/alias/note in session-meta.json

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Frontend meta library — types + pure helpers

**Files:**
- Create: `crates/webui/frontend/src/lib/sessionMeta.ts`
- Test: `crates/webui/frontend/src/lib/sessionMeta.test.ts`

**Interfaces:**
- Consumes: nothing (pure module).
- Produces: types `Tag { name; color }`, `SessionEntry { alias?; tags?; note? }`, `SessionMeta { version; tags: Tag[]; sessions: Record<string, SessionEntry> }`; `DEFAULT_META`; `TAG_PALETTE: string[]`; `tagColor(meta, name): string`; `entryTags(entry): string[]`; `matchesTagFilter(entry, selected: Set<string>): boolean`; pure mutators `setAliasIn`, `setNoteIn`, `setSessionTagsIn`, `upsertTagIn`, `removeTagIn`, `nextPaletteColor` — every mutator returns a NEW `SessionMeta` (no mutation) and prunes empty session entries.

- [ ] **Step 1: Write the failing tests**

Create `crates/webui/frontend/src/lib/sessionMeta.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import {
  DEFAULT_META,
  TAG_PALETTE,
  tagColor,
  entryTags,
  matchesTagFilter,
  setAliasIn,
  setNoteIn,
  setSessionTagsIn,
  upsertTagIn,
  removeTagIn,
  nextPaletteColor,
  type SessionMeta,
} from "./sessionMeta";

const base: SessionMeta = {
  version: 1,
  tags: [{ name: "wip", color: "#e5534b" }],
  sessions: { abc: { alias: "auth", tags: ["wip"], note: "n" } },
};

describe("tagColor", () => {
  it("resolves a registered tag's color", () => {
    expect(tagColor(base, "wip")).toBe("#e5534b");
  });
  it("falls back to neutral for an unknown tag", () => {
    expect(tagColor(base, "ghost")).toBe("#8b949e");
  });
});

describe("setAliasIn", () => {
  it("sets a trimmed alias", () => {
    const m = setAliasIn(DEFAULT_META, "x", "  hello  ");
    expect(m.sessions["x"].alias).toBe("hello");
  });
  it("clearing the alias prunes an otherwise-empty entry", () => {
    const m = setAliasIn(DEFAULT_META, "x", "hi");
    const m2 = setAliasIn(m, "x", "");
    expect(m2.sessions["x"]).toBeUndefined();
  });
  it("clearing the alias keeps an entry that still has tags", () => {
    const m = setSessionTagsIn(setAliasIn(DEFAULT_META, "x", "hi"), "x", ["wip"]);
    const m2 = setAliasIn(m, "x", null);
    expect(m2.sessions["x"]).toEqual({ tags: ["wip"] });
  });
  it("does not mutate the input", () => {
    const m = setAliasIn(base, "abc", "new");
    expect(base.sessions["abc"].alias).toBe("auth");
    expect(m.sessions["abc"].alias).toBe("new");
  });
});

describe("setNoteIn", () => {
  it("sets and clears a note (empty prunes when nothing else)", () => {
    const m = setNoteIn(DEFAULT_META, "x", "remember this");
    expect(m.sessions["x"].note).toBe("remember this");
    expect(setNoteIn(m, "x", "").sessions["x"]).toBeUndefined();
  });
});

describe("setSessionTagsIn", () => {
  it("sets tags and empty list prunes", () => {
    const m = setSessionTagsIn(DEFAULT_META, "x", ["a", "b"]);
    expect(entryTags(m.sessions["x"])).toEqual(["a", "b"]);
    expect(setSessionTagsIn(m, "x", []).sessions["x"]).toBeUndefined();
  });
});

describe("upsertTagIn", () => {
  it("creates a new tag", () => {
    const m = upsertTagIn(DEFAULT_META, "api", "#539bf5");
    expect(m.tags).toEqual([{ name: "api", color: "#539bf5" }]);
  });
  it("recolors an existing tag without duplicating", () => {
    const m = upsertTagIn(base, "wip", "#57ab5a");
    expect(m.tags).toEqual([{ name: "wip", color: "#57ab5a" }]);
  });
});

describe("removeTagIn", () => {
  it("removes from the registry AND from every session, pruning empties", () => {
    const m = removeTagIn(base, "wip");
    expect(m.tags).toEqual([]);
    // abc still has alias + note, so it survives without the tag.
    expect(m.sessions["abc"]).toEqual({ alias: "auth", note: "n" });
    const only: SessionMeta = {
      version: 1,
      tags: [{ name: "wip", color: "#e5534b" }],
      sessions: { z: { tags: ["wip"] } },
    };
    expect(removeTagIn(only, "wip").sessions["z"]).toBeUndefined();
  });
});

describe("nextPaletteColor", () => {
  it("picks the first palette color when registry is empty", () => {
    expect(nextPaletteColor(DEFAULT_META)).toBe(TAG_PALETTE[0]);
  });
  it("picks the least-used palette color", () => {
    const m: SessionMeta = {
      version: 1,
      tags: [
        { name: "a", color: TAG_PALETTE[0] },
        { name: "b", color: TAG_PALETTE[1] },
      ],
      sessions: {},
    };
    expect(nextPaletteColor(m)).toBe(TAG_PALETTE[2]);
  });
});

describe("matchesTagFilter", () => {
  it("empty selection matches everything", () => {
    expect(matchesTagFilter(undefined, new Set())).toBe(true);
  });
  it("matches when the entry has ANY selected tag", () => {
    expect(matchesTagFilter({ tags: ["wip", "api"] }, new Set(["api"]))).toBe(true);
    expect(matchesTagFilter({ tags: ["wip"] }, new Set(["api"]))).toBe(false);
    expect(matchesTagFilter(undefined, new Set(["api"]))).toBe(false);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd crates/webui/frontend && npx vitest run src/lib/sessionMeta.test.ts`
Expected: FAIL — cannot resolve `./sessionMeta`.

- [ ] **Step 3: Implement `sessionMeta.ts`**

Create `crates/webui/frontend/src/lib/sessionMeta.ts`:

```ts
// Claude-manager-local session organization: global colored tags + per-session
// alias/tags/note. Mirrors the Rust `session_meta` store; every mutator is pure
// (returns a new SessionMeta) and prunes entries with no remaining fields.

export interface Tag {
  name: string;
  color: string;
}

export interface SessionEntry {
  alias?: string | null;
  tags?: string[];
  note?: string | null;
}

export interface SessionMeta {
  version: number;
  tags: Tag[];
  sessions: Record<string, SessionEntry>;
}

export const DEFAULT_META: SessionMeta = { version: 1, tags: [], sessions: {} };

/** GitHub-label-ish colors that read on both themes (chips carry explicit color). */
export const TAG_PALETTE = [
  "#e5534b", // red
  "#f0883e", // orange
  "#d4a72c", // yellow
  "#57ab5a", // green
  "#39c5cf", // cyan
  "#539bf5", // blue
  "#986ee2", // purple
  "#e275ad", // pink
];

const FALLBACK_COLOR = "#8b949e"; // a tag name missing from the registry self-heals

export function tagColor(meta: SessionMeta, name: string): string {
  return meta.tags.find((t) => t.name === name)?.color ?? FALLBACK_COLOR;
}

export function entryTags(entry: SessionEntry | undefined): string[] {
  return entry?.tags ?? [];
}

/** Sidebar tag filter: empty selection matches all; else ANY-of semantics. */
export function matchesTagFilter(
  entry: SessionEntry | undefined,
  selected: Set<string>,
): boolean {
  if (selected.size === 0) return true;
  return entryTags(entry).some((t) => selected.has(t));
}

/** Least-used palette color, so new tags spread across the palette. */
export function nextPaletteColor(meta: SessionMeta): string {
  const counts = new Map(TAG_PALETTE.map((c) => [c, 0]));
  for (const t of meta.tags) counts.set(t.color, (counts.get(t.color) ?? 0) + 1);
  let best = TAG_PALETTE[0];
  for (const c of TAG_PALETTE) {
    if ((counts.get(c) ?? 0) < (counts.get(best) ?? 0)) best = c;
  }
  return best;
}

function withEntry(
  meta: SessionMeta,
  id: string,
  patch: (e: SessionEntry) => SessionEntry,
): SessionMeta {
  const cur = meta.sessions[id] ?? {};
  const next = patch({ ...cur });
  const sessions = { ...meta.sessions };
  // Prune: an entry with no alias, no tags, and no note is dropped entirely.
  if (!next.alias && !next.note && (next.tags?.length ?? 0) === 0) {
    delete sessions[id];
  } else {
    // Normalize away empty fields so the stored JSON stays minimal.
    const clean: SessionEntry = {};
    if (next.alias) clean.alias = next.alias;
    if (next.tags && next.tags.length > 0) clean.tags = next.tags;
    if (next.note) clean.note = next.note;
    sessions[id] = clean;
  }
  return { ...meta, sessions };
}

export function setAliasIn(meta: SessionMeta, id: string, alias: string | null): SessionMeta {
  const v = alias?.trim() || null;
  return withEntry(meta, id, (e) => ({ ...e, alias: v }));
}

export function setNoteIn(meta: SessionMeta, id: string, note: string | null): SessionMeta {
  const v = note?.trim() || null;
  return withEntry(meta, id, (e) => ({ ...e, note: v }));
}

export function setSessionTagsIn(meta: SessionMeta, id: string, names: string[]): SessionMeta {
  return withEntry(meta, id, (e) => ({ ...e, tags: names }));
}

/** Create a tag, or recolor it if the name already exists. */
export function upsertTagIn(meta: SessionMeta, name: string, color: string): SessionMeta {
  const tags = meta.tags.some((t) => t.name === name)
    ? meta.tags.map((t) => (t.name === name ? { ...t, color } : t))
    : [...meta.tags, { name, color }];
  return { ...meta, tags };
}

/** Delete a tag from the registry and strip it from every session (pruning). */
export function removeTagIn(meta: SessionMeta, name: string): SessionMeta {
  let next: SessionMeta = { ...meta, tags: meta.tags.filter((t) => t.name !== name) };
  for (const [id, e] of Object.entries(next.sessions)) {
    if (e.tags?.includes(name)) {
      next = setSessionTagsIn(next, id, (e.tags ?? []).filter((t) => t !== name));
    }
  }
  return next;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd crates/webui/frontend && npx vitest run src/lib/sessionMeta.test.ts`
Expected: PASS (all describe blocks).

Run: `npx tsc --noEmit` — Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/webui/frontend/src/lib/sessionMeta.ts crates/webui/frontend/src/lib/sessionMeta.test.ts
git commit -m "feat(webui): sessionMeta lib — types, palette, pure mutators

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `SessionMetaProvider` / `useSessionMeta` + mount

**Files:**
- Create: `crates/webui/frontend/src/hooks/useSessionMeta.tsx`
- Modify: `crates/webui/frontend/src/main.tsx`

**Interfaces:**
- Consumes: `get_session_meta` / `set_session_meta` commands (Task 1); `session-meta-changed` global event; the pure mutators from Task 2.
- Produces: `useSessionMeta(): { meta: SessionMeta; setAlias(id, alias): void; setNote(id, note): void; setSessionTags(id, names): void; upsertTag(name, color): void; removeTag(name): void }` — each mutator applies the pure fn, sets state optimistically, then persists via `set_session_meta` (echo-guarded broadcast sync, exactly like `useConfig`).

- [ ] **Step 1: Implement the provider**

Create `crates/webui/frontend/src/hooks/useSessionMeta.tsx`:

```tsx
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  DEFAULT_META,
  setAliasIn,
  setNoteIn,
  setSessionTagsIn,
  upsertTagIn,
  removeTagIn,
  type SessionMeta,
} from "../lib/sessionMeta";

interface SessionMetaCtx {
  meta: SessionMeta;
  setAlias: (id: string, alias: string | null) => void;
  setNote: (id: string, note: string | null) => void;
  setSessionTags: (id: string, names: string[]) => void;
  upsertTag: (name: string, color: string) => void;
  removeTag: (name: string) => void;
}

const Ctx = createContext<SessionMetaCtx>({
  meta: DEFAULT_META,
  setAlias: () => {},
  setNote: () => {},
  setSessionTags: () => {},
  upsertTag: () => {},
  removeTag: () => {},
});

export function useSessionMeta() {
  return useContext(Ctx);
}

export function SessionMetaProvider({ children }: { children: ReactNode }) {
  const [meta, setMeta] = useState<SessionMeta>(DEFAULT_META);
  const metaRef = useRef(meta);
  metaRef.current = meta;
  // JSON of our last write, so we ignore the broadcast echo of our own save.
  const lastWritten = useRef<string>("");

  useEffect(() => {
    let alive = true;
    invoke<SessionMeta>("get_session_meta")
      .then((m) => {
        if (alive) setMeta(m);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    const un = listen<SessionMeta>("session-meta-changed", (e) => {
      if (JSON.stringify(e.payload) === lastWritten.current) return; // our own echo
      setMeta(e.payload);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Apply a pure mutation, set optimistically, persist (broadcasts to others).
  const apply = useCallback((fn: (m: SessionMeta) => SessionMeta) => {
    const next = fn(metaRef.current);
    setMeta(next);
    lastWritten.current = JSON.stringify(next);
    invoke("set_session_meta", { meta: next }).catch(() => {});
  }, []);

  const value: SessionMetaCtx = {
    meta,
    setAlias: (id, alias) => apply((m) => setAliasIn(m, id, alias)),
    setNote: (id, note) => apply((m) => setNoteIn(m, id, note)),
    setSessionTags: (id, names) => apply((m) => setSessionTagsIn(m, id, names)),
    upsertTag: (name, color) => apply((m) => upsertTagIn(m, name, color)),
    removeTag: (name) => apply((m) => removeTagIn(m, name)),
  };

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
```

- [ ] **Step 2: Mount beside `ConfigProvider`**

In `crates/webui/frontend/src/main.tsx`:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ConfigProvider } from "./hooks/useConfig";
import { SessionMetaProvider } from "./hooks/useSessionMeta";
import "./index.css";
import "@xterm/xterm/css/xterm.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ConfigProvider>
      <SessionMetaProvider>
        <App />
      </SessionMetaProvider>
    </ConfigProvider>
  </React.StrictMode>
);
```

- [ ] **Step 3: Verify**

Run in `crates/webui/frontend`: `npx tsc --noEmit && npx vitest run && npx vite build`
Expected: tsc clean; all tests pass; build succeeds.

- [ ] **Step 4: Commit**

```bash
git add crates/webui/frontend/src/hooks/useSessionMeta.tsx crates/webui/frontend/src/main.tsx
git commit -m "feat(webui): SessionMetaProvider — load/apply/persist with cross-window sync

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Display join — alias, tag chips, note indicator

**Files:**
- Create: `crates/webui/frontend/src/components/TagChip.tsx`
- Modify: `crates/webui/frontend/src/components/Sidebar.tsx` (row title + tokens row)
- Modify: `crates/webui/frontend/src/components/Transcript.tsx` (header title + note under the details panel)

**Interfaces:**
- Consumes: `useSessionMeta()` (Task 3), `tagColor` / `entryTags` (Task 2).
- Produces: `TagChip({ name, color, onRemove? })` — shared by the sidebar rows and Task 5's `TagEditor`.

- [ ] **Step 1: Create `TagChip`**

Create `crates/webui/frontend/src/components/TagChip.tsx`:

```tsx
/** A small colored tag chip. Colors are explicit hex (theme-independent, like
 *  GitHub labels): tinted background + border derived from the tag color. */
export default function TagChip({
  name,
  color,
  onRemove,
}: {
  name: string;
  color: string;
  onRemove?: () => void;
}) {
  return (
    <span
      className="inline-flex max-w-full items-center gap-1 truncate rounded-full border px-1.5 py-0.5 text-[10px] leading-none"
      style={{ color, borderColor: `${color}66`, background: `${color}1f` }}
    >
      {name}
      {onRemove && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onRemove();
          }}
          className="opacity-70 hover:opacity-100"
          title={`Remove tag ${name}`}
        >
          ×
        </button>
      )}
    </span>
  );
}
```

- [ ] **Step 2: Sidebar — join + alias/chips/note on rows**

In `crates/webui/frontend/src/components/Sidebar.tsx`:

Add imports:

```tsx
import TagChip from "./TagChip";
import { useSessionMeta } from "../hooks/useSessionMeta";
import { entryTags, matchesTagFilter, tagColor } from "../lib/sessionMeta";
```

Inside the `Sidebar` component body (near the other hooks at the top):

```tsx
  const { meta } = useSessionMeta();
```

In the row `sorted.map((s) => {` callback, next to the existing `const act = ...` line, add:

```tsx
            const entry = meta.sessions[s.id];
```

Replace the row title block:

```tsx
                  <div
                    className={`text-sm text-[var(--color-text)] truncate mt-0.5 ${
                      unread ? "font-semibold" : ""
                    }`}
                  >
                    {s.title}
                  </div>
```

with alias-primary / claude-name-dim:

```tsx
                  {entry?.alias ? (
                    <>
                      <div
                        className={`text-sm text-[var(--color-text)] truncate mt-0.5 ${
                          unread ? "font-semibold" : ""
                        }`}
                      >
                        {entry.alias}
                      </div>
                      <div className="text-[11px] text-[var(--color-text-dim)] truncate">
                        {s.title}
                      </div>
                    </>
                  ) : (
                    <div
                      className={`text-sm text-[var(--color-text)] truncate mt-0.5 ${
                        unread ? "font-semibold" : ""
                      }`}
                    >
                      {s.title}
                    </div>
                  )}
```

In the tokens row (`<div className="flex items-center gap-2 text-[10px] ...">` holding `{compactTokens(...)}`), append a note indicator as the LAST child inside that row:

```tsx
                    {entry?.note && (
                      <span title={entry.note} className="shrink-0">
                        ✎
                      </span>
                    )}
```

Immediately AFTER that tokens-row `</div>`, add the chips row:

```tsx
                  {entryTags(entry).length > 0 && (
                    <div className="mt-1 flex flex-wrap gap-1">
                      {entryTags(entry).map((name) => (
                        <TagChip key={name} name={name} color={tagColor(meta, name)} />
                      ))}
                    </div>
                  )}
```

- [ ] **Step 3: Transcript — alias in the header + note under details**

In `crates/webui/frontend/src/components/Transcript.tsx`:

Add imports:

```tsx
import { useSessionMeta } from "../hooks/useSessionMeta";
```

Inside the `Transcript` component, next to the `useConfig()` line:

```tsx
  const { meta } = useSessionMeta();
  const metaEntry = meta.sessions[session.id];
```

Replace the header title span:

```tsx
        <span className="text-[var(--color-text)] font-medium truncate">
          {session.title}
        </span>
```

with:

```tsx
        <span className="text-[var(--color-text)] font-medium truncate">
          {metaEntry?.alias ?? session.title}
        </span>
        {metaEntry?.alias && (
          <span className="text-xs text-[var(--color-text-dim)] truncate">
            {session.title}
          </span>
        )}
```

Replace the details-panel render:

```tsx
      {statsOpen && (
        <StatsPanel
          stats={stats}
          eventCount={session.event_count}
          sessionId={session.id}
        />
      )}
```

with:

```tsx
      {statsOpen && (
        <>
          <StatsPanel
            stats={stats}
            eventCount={session.event_count}
            sessionId={session.id}
          />
          {metaEntry?.note && (
            <div className="border-b border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-2 text-xs text-[var(--color-text-dim)] whitespace-pre-wrap break-words">
              <span className="text-[var(--color-text)] font-medium">Note:</span>{" "}
              {metaEntry.note}
            </div>
          )}
        </>
      )}
```

- [ ] **Step 4: Verify**

Run in `crates/webui/frontend`: `npx tsc --noEmit && npx vitest run && npx vite build`
Expected: all green (display-only change; behavior verified visually by the user via HMR).

- [ ] **Step 5: Commit**

```bash
git add crates/webui/frontend/src/components/TagChip.tsx crates/webui/frontend/src/components/Sidebar.tsx crates/webui/frontend/src/components/Transcript.tsx
git commit -m "feat(webui): show alias (primary, claude name dim), tag chips, note

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Editors — inline Rename, TagEditor, NoteEditor + row menu

**Files:**
- Create: `crates/webui/frontend/src/components/TagEditor.tsx`
- Create: `crates/webui/frontend/src/components/NoteEditor.tsx`
- Modify: `crates/webui/frontend/src/components/Sidebar.tsx` (menu items + inline rename + modal state)

**Interfaces:**
- Consumes: `useSessionMeta()` mutators; `TagChip` (Task 4); `menuItemClass` from `Dropdown.tsx`; `TAG_PALETTE`/`nextPaletteColor`/`tagColor`/`entryTags` (Task 2).
- Produces: `TagEditor({ sessionId, title, onClose })`, `NoteEditor({ sessionId, title, onClose })` — both centered modal overlays (Esc/backdrop close), self-sufficient via `useSessionMeta`.

- [ ] **Step 1: Create `TagEditor`**

Create `crates/webui/frontend/src/components/TagEditor.tsx`:

```tsx
import { useEffect, useMemo, useState } from "react";
import TagChip from "./TagChip";
import { useSessionMeta } from "../hooks/useSessionMeta";
import { entryTags, nextPaletteColor, tagColor } from "../lib/sessionMeta";

/** Edit a session's tags: current tags as removable chips + an input that
 *  autocompletes registry tags; a new name is created with the next palette
 *  color. Centered modal, Esc/backdrop close. */
export default function TagEditor({
  sessionId,
  title,
  onClose,
}: {
  sessionId: string;
  title: string;
  onClose: () => void;
}) {
  const { meta, setSessionTags, upsertTag } = useSessionMeta();
  const [input, setInput] = useState("");
  const current = entryTags(meta.sessions[sessionId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const suggestions = useMemo(() => {
    const q = input.trim().toLowerCase();
    return meta.tags
      .filter((t) => !current.includes(t.name))
      .filter((t) => !q || t.name.toLowerCase().includes(q))
      .slice(0, 8);
  }, [meta.tags, current, input]);

  const addTag = (name: string) => {
    const n = name.trim();
    if (!n || current.includes(n)) return;
    if (!meta.tags.some((t) => t.name === n)) {
      upsertTag(n, nextPaletteColor(meta));
    }
    setSessionTags(sessionId, [...current, n]);
    setInput("");
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-full max-w-sm rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl space-y-3"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-[var(--color-text)] truncate">
            Tags — {title}
          </span>
          <button
            onClick={onClose}
            className="text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
          >
            ×
          </button>
        </div>

        <div className="flex flex-wrap gap-1 min-h-6">
          {current.length === 0 && (
            <span className="text-xs text-[var(--color-text-dim)]">No tags yet</span>
          )}
          {current.map((name) => (
            <TagChip
              key={name}
              name={name}
              color={tagColor(meta, name)}
              onRemove={() =>
                setSessionTags(
                  sessionId,
                  current.filter((t) => t !== name),
                )
              }
            />
          ))}
        </div>

        <input
          autoFocus
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addTag(input);
            }
          }}
          placeholder="Add a tag (Enter creates a new one)…"
          className="w-full bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-2 py-1.5 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
        />

        {suggestions.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {suggestions.map((t) => (
              <button key={t.name} onClick={() => addTag(t.name)} title="Add this tag">
                <TagChip name={t.name} color={t.color} />
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Create `NoteEditor`**

Create `crates/webui/frontend/src/components/NoteEditor.tsx`:

```tsx
import { useEffect, useState } from "react";
import { useSessionMeta } from "../hooks/useSessionMeta";

/** Edit a session's free-text note. Save persists (empty clears); Esc/backdrop
 *  cancels without saving. */
export default function NoteEditor({
  sessionId,
  title,
  onClose,
}: {
  sessionId: string;
  title: string;
  onClose: () => void;
}) {
  const { meta, setNote } = useSessionMeta();
  const [draft, setDraft] = useState(meta.sessions[sessionId]?.note ?? "");

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const save = () => {
    setNote(sessionId, draft);
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-full max-w-sm rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl space-y-3"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-[var(--color-text)] truncate">
            Note — {title}
          </span>
          <button
            onClick={onClose}
            className="text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
          >
            ×
          </button>
        </div>
        <textarea
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          rows={5}
          placeholder="What is this session about? (empty clears the note)"
          className="w-full resize-y bg-[var(--color-bg)] text-[var(--color-text)] text-xs px-2 py-1.5 rounded border border-[var(--color-border)] outline-none focus:border-[var(--color-accent)]"
        />
        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="text-xs px-3 py-1 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] hover:text-[var(--color-text)]"
          >
            Cancel
          </button>
          <button
            onClick={save}
            className="text-xs px-3 py-1 rounded bg-[var(--color-accent)] text-white"
          >
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 3: Sidebar — menu items, inline rename, modal state**

In `crates/webui/frontend/src/components/Sidebar.tsx`:

Extend imports (Dropdown line gains `menuItemClass`; add the new components and hook pieces):

```tsx
import Dropdown, { menuItemClass, ConfirmMenuItem } from "./Dropdown";
import TagEditor from "./TagEditor";
import NoteEditor from "./NoteEditor";
```

Add state next to `deletingId`:

```tsx
  const { meta, setAlias } = useSessionMeta(); // replaces the plain `{ meta }` destructure from Task 4
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [draftAlias, setDraftAlias] = useState("");
  const [tagsFor, setTagsFor] = useState<SessionSummary | null>(null);
  const [noteFor, setNoteFor] = useState<SessionSummary | null>(null);
```

In the row render, define `renaming` beside `entry`:

```tsx
            const entry = meta.sessions[s.id];
            const renaming = renamingId === s.id;
```

Wrap the main clickable button in the rename conditional — the current structure is `{/* Main clickable area */}` followed by `<button onClick={() => onSelect(s)} ...>…</button>`; it becomes:

```tsx
              {renaming ? (
                /* Inline alias input (an input can't live inside the row button) */
                <div className="flex-1 px-3 py-2 min-w-0 flex items-center gap-2">
                  <input
                    autoFocus
                    value={draftAlias}
                    onChange={(e) => setDraftAlias(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        setAlias(s.id, draftAlias); // trimmed-empty clears
                        setRenamingId(null);
                      } else if (e.key === "Escape") {
                        e.preventDefault();
                        setRenamingId(null);
                      }
                    }}
                    onBlur={() => setRenamingId(null)}
                    placeholder="Alias (leave empty to clear)"
                    className="w-full bg-[var(--color-bg)] text-[var(--color-text)] text-sm px-1.5 py-1 rounded border border-[var(--color-accent)] outline-none"
                  />
                </div>
              ) : (
                <button
                  onClick={() => onSelect(s)}
                  className="flex-1 text-left px-3 py-2 min-w-0"
                >
                  {/* …existing row content unchanged… */}
                </button>
              )}
```

In the row `⋯` Dropdown children, add three items BEFORE `<ConfirmMenuItem …/>`:

```tsx
                        <button
                          role="menuitem"
                          className={menuItemClass}
                          onClick={(e) => {
                            e.stopPropagation();
                            close();
                            setDraftAlias(entry?.alias ?? "");
                            setRenamingId(s.id);
                          }}
                        >
                          Rename
                        </button>
                        <button
                          role="menuitem"
                          className={menuItemClass}
                          onClick={(e) => {
                            e.stopPropagation();
                            close();
                            setTagsFor(s);
                          }}
                        >
                          Tags…
                        </button>
                        <button
                          role="menuitem"
                          className={menuItemClass}
                          onClick={(e) => {
                            e.stopPropagation();
                            close();
                            setNoteFor(s);
                          }}
                        >
                          Note…
                        </button>
```

At the END of the `<aside>` (just before `</aside>`), render the modals:

```tsx
      {tagsFor && (
        <TagEditor
          sessionId={tagsFor.id}
          title={meta.sessions[tagsFor.id]?.alias ?? tagsFor.title}
          onClose={() => setTagsFor(null)}
        />
      )}
      {noteFor && (
        <NoteEditor
          sessionId={noteFor.id}
          title={meta.sessions[noteFor.id]?.alias ?? noteFor.title}
          onClose={() => setNoteFor(null)}
        />
      )}
```

Also update the Task-4 comment above the `⋯` menu from `{/* Per-row ⋯ menu (Delete) */}` to `{/* Per-row ⋯ menu: Rename / Tags / Note / Delete */}`, and hide the menu while renaming this row (mirror the old `!editing` guard):

```tsx
              {!renaming && (
                <div className={`flex items-center shrink-0 pr-1.5 transition-opacity ${deletingId === s.id ? "opacity-100" : "opacity-0 group-hover:opacity-100"}`}>
                  …existing Dropdown…
                </div>
              )}
```

- [ ] **Step 4: Verify**

Run in `crates/webui/frontend`: `npx tsc --noEmit && npx vitest run && npx vite build`
Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add crates/webui/frontend/src/components/TagEditor.tsx crates/webui/frontend/src/components/NoteEditor.tsx crates/webui/frontend/src/components/Sidebar.tsx
git commit -m "feat(webui): Rename/Tags…/Note… in the row menu (alias inline, modals)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Sidebar tag filter

**Files:**
- Modify: `crates/webui/frontend/src/components/Sidebar.tsx` (filter row + `filtered` memo)

**Interfaces:**
- Consumes: `matchesTagFilter(entry, selected)` (Task 2, already tested); `Dropdown`; `TagChip`.
- Produces: nothing new (self-contained UI state).

- [ ] **Step 1: Add filter state + predicate**

In `Sidebar.tsx`, state next to the other filter state:

```tsx
  const [tagFilter, setTagFilter] = useState<Set<string>>(new Set());
```

In the `filtered` memo, after the project/directory checks:

```tsx
      // Tag filter (any-of): only sessions carrying at least one selected tag.
      if (!matchesTagFilter(meta.sessions[s.id], tagFilter)) return false;
```

and extend the memo's dependency array with `meta` and `tagFilter`.

- [ ] **Step 2: Add the filter control**

In the filters row (after the date-range `<select>`), shown only when tags exist:

```tsx
          {meta.tags.length > 0 && (
            <Dropdown
              trigger={tagFilter.size > 0 ? `tags · ${tagFilter.size}` : "tags"}
              title="Filter by tag"
              triggerClassName={`text-xs px-1.5 py-1 rounded border outline-none ${
                tagFilter.size > 0
                  ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                  : "border-[var(--color-border)] text-[var(--color-text)] bg-[var(--color-bg)]"
              }`}
            >
              {() => (
                <>
                  {meta.tags.map((t) => {
                    const on = tagFilter.has(t.name);
                    return (
                      <button
                        key={t.name}
                        role="menuitem"
                        className={`${menuItemClass} flex items-center gap-2`}
                        onClick={(e) => {
                          e.stopPropagation();
                          setTagFilter((prev) => {
                            const next = new Set(prev);
                            if (next.has(t.name)) next.delete(t.name);
                            else next.add(t.name);
                            return next;
                          });
                        }}
                      >
                        <span className={on ? "" : "opacity-0"}>✓</span>
                        <TagChip name={t.name} color={t.color} />
                      </button>
                    );
                  })}
                  {tagFilter.size > 0 && (
                    <button
                      role="menuitem"
                      className={`${menuItemClass} text-[var(--color-text-dim)]`}
                      onClick={(e) => {
                        e.stopPropagation();
                        setTagFilter(new Set());
                      }}
                    >
                      Clear filter
                    </button>
                  )}
                </>
              )}
            </Dropdown>
          )}
```

- [ ] **Step 3: Verify**

Run in `crates/webui/frontend`: `npx tsc --noEmit && npx vitest run && npx vite build`
Expected: all green (`matchesTagFilter` semantics already covered by Task 2 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/webui/frontend/src/components/Sidebar.tsx
git commit -m "feat(webui): sidebar tag filter (any-of, combinable with project/date)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Full verification + wrap-up

**Files:**
- Modify: `TODOS.md` (mark items done — leave UNCOMMITTED, per convention)

**Interfaces:** none.

- [ ] **Step 1: Full suites**

```bash
cargo test 2>&1 | grep -E "test result|error"
cd crates/webui/frontend && npx tsc --noEmit && npx vitest run && npx vite build
```

Expected: 107 core + 3 tui + 18 webui Rust tests; frontend tests all pass (≈40 + the new sessionMeta suite); tsc clean; build succeeds.

- [ ] **Step 2: Update TODOS.md (uncommitted)**

Check off the two "Session organization" items with short what-shipped notes (tags/alias/note/filter; separate `session-meta.json`; alias-primary display).

- [ ] **Step 3: Hand off for user verification + merge decision**

Remind the user: backend changed → restart `make webui` to get `get/set_session_meta`. Present finish options (merge is user-directed; FEATURES.md entry happens on the branch at merge time per convention).
