# Appearance modes — design

**Date:** 2026-11-10
**TODO:** "different appearance modes, which specify how much detail is shown in chat. for example, show all tools full input and output, show only input/output, show truncated input/output. should be entirely customizable"

## Problem

How much of a transcript the app shows is fixed in code. Every tool call starts collapsed and one click opens its input and output together (`components/ToolCall.tsx`, `useState(false)` at the top of `ToolCall`). An opened input sits in a 300px scroll box and an opened output in a 400px one. Task notifications start collapsed, with their result in a 400px box (`TaskNotificationBlock` in `components/Transcript.tsx`). Narration (`· summarized`), system lines and the compaction summary always show in full, and `!` shell output sits in a 400px box (the `bash` case of `Turn`). The only lever is the header's **Compact** button (`lib/compactView.ts` `filterCompact`), which keeps prompts plus the first and last assistant turn of each reply.

Reading a long session means either clicking open dozens of tool calls one at a time or seeing none of them. Skimming just the conversation means scrolling past every tool row.

## Intent

A window shows the transcript in one named **appearance mode**. A mode is one complete set of display levels, one per kind of content. Five built-in modes cover the common reading styles, one of which is today's view, unchanged. The header offers a single dropdown to switch modes, ⌘⇧D cycles through them, and Settings picks the mode a new window starts in.

## Goals

- Five presets, each switchable instantly per window: **Default**, **Conversation**, **Inputs**, **Truncated**, **Everything**.
- Every preset is a value of one typed structure that names every knob, so a mode editor can be added later without reshaping it.
- The Compact button goes away; **Conversation** covers its use, skimming the exchange without the tool work.
- Clamped output stays fully searchable, and in-session find opens whatever it lands in.

## Non-goals (deferred, not built and not stubbed)

- A mode editor, user-created modes, renaming or duplicating modes.
- Per-tool override rules (e.g. "Bash output full, everything else collapsed").
- Storing custom modes. Config holds only the default mode's id, with no `modes` array.
- Remembering a window's mode across restarts, or a mode per session.
- A knob for real thinking. It is persisted empty: in the 40 most recent local sessions, 2647 thinking blocks were empty and the 295 non-empty ones were narration. `BlockRenderer` returns `null` for `thinking` and keeps doing so.

## Anti-goals (fail even if it works)

- **Default looking any different from today.** A user who never opens the dropdown must not see a pixel change: the same collapsed rows, the same 300/400px boxes, one click opening input and output together.
- **Find breaking inside clamped output.** A match below a clamp must be counted, navigable and painted at the right characters. Clamping that slices text instead of hiding it with CSS is rejected in review.

## The mode structure

A new pure module, `crates/app/frontend/src/lib/appearance.ts` (no DOM, no React):

```ts
/** How tall an open body may grow. `scroll` is today's fixed box for that kind (see SCROLL_PX). */
export type Height = { kind: "scroll" } | { kind: "clamp"; lines: number } | { kind: "full" };

/** One kind of content: whether it starts shown, and how tall it may be once open. */
export interface Part<Show extends string> {
  show: Show;
  height: Height;
}

export interface AppearanceMode {
  id: string;                                            // "default" | "conversation" | … for the built-ins
  name: string;                                          // label in the dropdown and the jump banner
  description: string;                                   // one dim line in the dropdown
  toolCalls: "shown" | "hidden";                         // the whole tool row
  toolInput: Part<"collapsed" | "open">;                 // non-diff input: Bash snippet, question card, JSON
  toolOutput: Part<"collapsed" | "open">;                // result text + returned images, or "(no result)"
  diffs: Part<"collapsed" | "open">;                     // Edit/Write/MultiEdit input and a Bash call's file changes
  taskNotifications: Part<"hidden" | "collapsed" | "open">;
  compactionSummary: Part<"hidden" | "open">;
  shellOutput: Height;                                   // `!` bash-mode stdout/stderr
  narration: "shown" | "hidden";
  systemLines: "shown" | "hidden";
}

export const SCROLL_PX = { toolInput: 300, toolOutput: 400, diffs: 300, taskNotifications: 400, compactionSummary: 400, shellOutput: 400 } as const;
export const MODES: readonly AppearanceMode[];           // the five presets, frozen, in cycle order
export const DEFAULT_MODE: AppearanceMode;               // MODES[0]
export function modeById(id: string): AppearanceMode;    // unknown id → DEFAULT_MODE
export function nextMode(id: string): AppearanceMode;    // ⌘⇧D order, wrapping
export function applyMode(turns: RenderedTurn[], mode: AppearanceMode): RenderedTurn[];
```

The `SCROLL_PX` values are today's constants: ToolCall.tsx input box 300 and result box 400, the `FileChanges.tsx` `FileDiff` box 300, task result 400, bash-mode output 400. Only `compactionSummary` has no box today and no preset uses `scroll` for it. Its entry exists so `Height` stays one type for every kind.

A part a mode marks `hidden` exists only where the whole row or turn can disappear. Per-part hiding inside a shown tool call (e.g. input shown, output hidden) is left out: no preset needs it, and it would change what find counts. Widening `Part<"collapsed" | "open">` to admit `"hidden"` later is additive.

## Presets

Clamp line counts were chosen from measured line counts over the 40 most recent local sessions. Tool results: p50 7, p90 90, p99 403, max 1426 (5060 results). Bash inputs: p50 2, p90 17, p99 76. JSON inputs: p50 4, p90 8, p99 26. Edit/Write/MultiEdit diffs: p50 6, p75 25, p90 57, p99 279, max 1473. Task notifications: p50 18, p90 125. Each clamp leaves the typical body whole and cuts the long tail.

| Knob | Default | Conversation | Inputs | Truncated | Everything |
|---|---|---|---|---|---|
| toolCalls | shown | **hidden** | shown | shown | shown |
| toolInput | collapsed · scroll | collapsed · scroll | **open** · scroll | **open · clamp 12** | **open · full** |
| toolOutput | collapsed · scroll | collapsed · scroll | collapsed · scroll | **open · clamp 15** | **open · full** |
| diffs | collapsed · scroll | collapsed · scroll | **open** · scroll | **open · clamp 30** | **open · full** |
| taskNotifications | collapsed · scroll | **hidden** | collapsed · scroll | **open · clamp 15** | **open · full** |
| compactionSummary | open · full | open · full | open · full | open · **clamp 20** | open · full |
| shellOutput | scroll | scroll | scroll | **clamp 15** | **full** |
| narration | shown | **hidden** | shown | shown | shown |
| systemLines | shown | **hidden** | shown | shown | shown |

Bold marks a difference from Default. Conversation's tool knobs are inert because its tool rows are hidden, but they still hold Default's values so that every preset is a complete set.

Dropdown descriptions: Default "Tool calls collapsed", Conversation "Prose only", Inputs "Tool inputs open", Truncated "Everything open, clipped to a few lines", Everything "Everything open, in full".

## What each level means, per kind of content

**Levels.** `collapsed` means the content is behind a ▸ toggle and not in the DOM, as today. `open` means it is rendered on mount. `hidden` means it is not rendered and not searched. A `scroll` height is today's `maxHeight: <SCROLL_PX>px; overflowY: auto` box. `clamp N` is described under Clamping below. `full` means no max-height and no inner scroll box, so the body grows to its natural height inside the page scroller.

**Tool call row** (`ToolCall.tsx`). `toolCalls: "hidden"` makes `BlockRenderer` return `null` for `tool_call` blocks, the way it does for `thinking`. Otherwise the header row is unchanged in every mode: chevron, `prettyToolName`, `input_summary`, and `ChangesBadge`.

**Tool input.** This is the `ToolInput` `<pre data-seg-id="…:tool-input">`. When `toolDiff(name, input)` is non-null (Edit/Write/MultiEdit), the input renders as `DiffBody` and follows the `diffs` knob. Otherwise it follows `toolInput`: the Bash snippet (`BashBody`), the question card (`AskBody`), or `toolInputText` JSON.

**Tool output.** This is the `ToolResult` box: the `<pre data-seg-id="…:tool-result">` with `AnsiText` over `prettyResultText(result.text)`, followed by the `ImageStrip` of returned images. It follows `toolOutput`, and so does the "(no result)" placeholder. ANSI colours render through `AnsiText` exactly as now, and clamping never touches the text. The height applies to the outer box, as `maxHeight` does today, so returned images sit inside the same box. Under a clamp they can fall below the fold, and "show all" brings them back.

**Bash file changes.** This is `FileChangesBody` (`…:tool-changes`), rendered after the result. It follows `diffs`, both its show and its height. Each `FileDiff` `<pre>` takes the height that today is its fixed 300px. A clamp of 30 applies to each file's pre separately.

**Task notifications** (`TaskNotificationBlock`). `hidden` makes `applyMode` drop the turn. Otherwise the summary row always shows, and `collapsed`/`open` sets the initial state of the result `<pre>` (`…:task-result`), with the height applied to it.

**Compaction summary** (the `summary` turn). `hidden` drops the turn, and `open` renders it with the height applied to its content `div` (`…:summary`). The `compaction` divider ("context compacted · N messages above") is never affected by any mode.

**Narration.** `hidden` makes `BlockRenderer` return `null` for `narration` blocks.

**System lines.** `hidden` makes `applyMode` drop `system` turns. `api_error` turns are not system lines: they explain why a reply stopped, so they show in every mode.

**`!` shell turns.** The command always shows. `shellOutput` sets the height of the stdout/stderr box that today is fixed at 400px. Shell turns are user input, so even Conversation keeps them.

**Images.** User-attached images (`ImageStrip` under the user bubble) and image paths in assistant markdown (`components/PathImage.tsx`) are prose, and no mode touches them. Tool-returned images go with the tool output, as described above.

**Untouched by every mode.** User text, assistant text, insight callouts, command turns, `api_error`, compaction dividers, timestamps, model/effort labels and turn actions.

### `applyMode(turns, mode)`

1. When the mode hides nothing that can drop a turn, return `turns` itself, the same array. That covers Default, Inputs, Truncated and Everything.
2. Otherwise filter, keeping the original turn objects (no copies). Drop `system` when `systemLines` is hidden, `task_notification` when `taskNotifications` is hidden, and `summary` when `compactionSummary` is hidden. Drop an `assistant` turn only if the mode hides at least one of its blocks and none of its blocks remains visible. A visible block is a `text` block, a `narration` block while narration is shown, or a `tool_call` while tool calls are shown. A thinking-only turn hides no block, so it renders as it does today (Default must not change).

Blocks are never removed from a turn. Hidden blocks render `null`, so `blockIndex`, seg ids and find's reveal keys stay the same in every mode. If the approved turn filter (`2026-08-05-turn-filter-design.md`, not yet built) is added later, it runs before `applyMode`, as its spec puts it before any view filter.

## Clamping

Clamping is visual only. The text node holds the whole body, and `max-height: calc(N * 1lh + <vertical padding>)` with `overflow: hidden` goes on the same box that holds `maxHeight` today. The padding is given in the box's own CSS units (`0.5rem` for `py-2`), not pixels, so it holds at any font size. When the body overflows, the box drops its bottom padding and its max-height counts only the top padding, so it ends exactly on a line boundary; otherwise the hidden text paints into the bottom padding and shows the top of line N+1. The `lh` unit needs WebKit 16.4. The builder confirms it renders in the agent app; if it doesn't, the fallback is N × the element's computed `line-height`. When the box overflows (measured with a `ResizeObserver`: `scrollHeight > clientHeight`), a bottom fade about 1.5 lines tall from transparent to `--color-tool-result-bg` overlays the box, and a small dim **show all N lines** button follows it. N is the logical line count of the rendered text, its `\n` count + 1, computed from the same string find uses (`toolInputText`, `toolResultText`, `fileChangesText`, `ansiText(result)`, the summary content, and stdout + stderr), memoised. The clamp cuts wrapped lines, so when N is no larger than the clamp the label is just **show all**: a few very long lines would otherwise offer fewer lines than already show. Clicking the button lifts the clamp, and the button becomes **show less**. Clicking **show less** folds the body away above the button, so the transcript scrolls by the same amount to keep the button where it was on screen. A body that fits shows neither the fade nor the button.

The fade and the button must sit **outside** the `data-seg-id` element. That element's `textContent` has to equal the model text exactly: find paints model-side matches by offset into it (the contract documented on `DiffBody`, `AskBody`, `BashBody` and `FileChangesBody`). The one exception is a Bash call's file changes, whose per-file diff boxes sit inside the `…:tool-changes` element; there the fade has no text and the button's label is CSS `content` (`.clamp-toggle::before`), so neither adds to `textContent`. The button uses that label everywhere, for consistency.

This is the orchestrator's call, made for find correctness: any text slicing would shift offsets, and matches below the cut would vanish.

## Open/closed state and user clicks

A tool call has up to three parts: input, output, and changes (when present). Each part's initial `open` comes from its knob (`diffs` for a diff input and for changes, `toolInput`/`toolOutput` otherwise). Each part's clamp starts on when its height is `clamp`.

- **Header chevron.** It shows ▾ when any part is open. Clicking it closes every part when any is open, and opens every part otherwise. In Default every part starts collapsed, so one click opens input and output together, exactly as today.
- **Part sub-rows.** While the header is ▾, each part that is closed shows a dim one-line toggle where its body would be, such as `▸ output · 93 lines` or `▸ changes · 2 files`, and clicking it opens just that part. This only arises in modes that open some parts and not others (Inputs). In Default, Truncated and Everything all parts share one state, so no sub-row ever renders, and Default's markup stays identical.
- **Task notification.** It keeps its single toggle, initialised from the knob.
- **Mode switch resets clicks.** The window counts its mode switches (`modeSeq` in `lib/windowView.ts`, bumped whenever the mode id changes). A component keeps its clicks, and a clamp its "show all", together with the count they were made under. When the window's mode changes, they no longer apply and every part takes the new mode's levels. Switching back does not restore them, because the count has moved on. The mode switch is a request to see everything at that level. Implemented as a stored count comparison, not by remounting the transcript.
- **Live tail.** A newly arriving tool call starts at the current mode's levels. Turns are keyed by their first uuid (falling back to the index for turns without one), so a turn that takes another's index gets fresh components instead of the old turn's clicks.

## Find, jump and bookmarks

- **What find sees.** `useFind` keeps receiving the rendered turn list, now `applyMode`'s output (`Transcript.tsx` `visibleTurns`). DOM-sourced fields (narration, system, summary) disappear from find when hidden, because they are not rendered. `modelMatches` (`lib/findMatches.ts`) gets the mode's `toolCalls` value and skips `tool_call` blocks when it is `"hidden"`, because those blocks are still in the turn. The ⚒ tools toggle (`opts.tools`) keeps its meaning: off, tool bodies are not searched at all, open or not.
- **Collapsed or clamped hits open.** Stepping onto a match puts its `revealKey` into `revealedKeys`, and the receiving component gets `forceExpand` (unchanged plumbing). `forceExpand` now means: open every part of that tool call and lift every clamp in it. A task notification opens its result and lifts its clamp. When the reveal set drops the key, the component goes back to its mode/click state, as today. Reveal keys name turns by index, so whenever the mode changes or the rendered list is reindexed (anything other than a live-tail append), the set is reset to just the active match's key under the new indexing. A mode switch with the bar open also repaints the highlights and the overview ruler, because it can remount matched segments (a body moving into or out of a clamp) or mount new ones without changing any match; that repaint does not scroll. The compaction summary is DOM-sourced, so its clamped text is matched from the DOM. `revealKey` gains `"<ti>:summary"` for the `summary` field, and the `summary` case of `Turn` passes `forceExpand` from it, so stepping onto a summary match lifts that clamp. Shell output has no `data-seg-id` today, so find doesn't search it and it needs no reveal.
- **A jump to a hidden message.** `lib/jumpTarget.ts` renames `compact-hidden` to `mode-hidden`: rendered by the session but filtered out by the window's mode because its kind is hidden. The existing miss banner (`Transcript.tsx`, `jumpMiss`) reads "That message is hidden by ‹mode name› mode." and its button becomes **Show in Default mode**. The button switches this window to Default (no config write) and bumps `rejump`, as "Show all" does today. Bookmark jumps (sidebar ⚑ browser) and global-find jumps both go through `locateJump`, so both get this behaviour. This is the orchestrator's call.
- **Known limit.** A global-find hit inside a hidden tool call lands on its turn, which is visible because it has text. The seeded find bar then has no match there. The jump is not reported as a miss, because the turn is on screen.

## Selection UI and shortcut

- **Header.** The Compact button in the `Transcript.tsx` header becomes a `Dropdown` (`components/Dropdown.tsx`, `align="right"`). Its trigger shows the active mode's name and uses the Compact button's classes: accent border and text when the mode is not Default, dim border and text when it is. Its title is "Appearance mode (⌘⇧D)". Each `menuItemClass` item shows the mode name with its description dimmed beneath, and a ✓ on the active mode. Picking an item sets this window's mode and closes the menu. The Details button stays exactly as it is.
- **⌘⇧D** cycles forward through `MODES` and wraps. It goes in `App.tsx`'s capture-phase keydown handler, beside ⌘⇧F, and is ignored while the Terminal tab is showing (the same rule as ⌘F in `Transcript.tsx`). The shortcut is free: the App handler takes ⌘⇧F, ⌘E, ⌘B, ⌘, and ⌘1–4, `Transcript.tsx` takes ⌘F, `hooks/useFind.ts` takes ⌘G/⌘⇧G, and the app menu (`crates/app/src/lib.rs` `build_menu`, `dev_reload.rs`) has ⌘Q, ⌘N, ⌘⇧R, close-window and the predefined edit/window items. The orchestrator chose the shortcut; this design confirmed it is unused.
- **Viewport.** A mode switch keeps the turn at the top of the viewport in place. Measure it with `viewAnchor` before the switch and restore it after layout, as `scheduleRestore` does. If that turn is hidden by the new mode, fall back to the existing length-change effect (re-pin to the bottom when pinned).

## Per-window state and seeding

`lib/windowView.ts` replaces `compact: boolean` with `modeId: string`. `detailsOpen` and its behaviour are unchanged, including that toggling Details still writes `ui.detailsOpen` as the last-used value. `seedWindowView` seeds `modeId` from `modeById(cfg.appearance.defaultMode).id` once config has loaded. `hooks/useWindowView.tsx` exposes `mode: AppearanceMode` (resolved through `modeById`, so its identity is a stable preset constant) and `setMode(id)`. `setMode` updates only this window's state and never calls `setConfig`, so other windows, and the next launch, are untouched. After seeding, a config broadcast from another window does not change this window's mode, which follows the existing `seeded` rule. A new window, an app restart and a webview reload each start in the configured default. These are the user's decisions: the mode is per window and is not remembered across restarts.

## Settings

In `components/Settings.tsx`, the Behavior section's "Compact transcript" `CheckRow` is replaced by a `Row` labelled **Default mode for new windows**. It holds a `Dropdown` with the same five items, styled like "Default project on launch", and writes `setConfig({ appearance: { defaultMode: id } })`. It does not change the current window's mode. "Show details panel" stays. Settings' doc comment, which names Compact and Details as the per-window controls, is updated to name the mode and Details.

## Config and migration

- **Rust** (`crates/app/src/config.rs`): `Config` gains `#[serde(default)] pub appearance: Appearance`, and `Appearance` holds `#[serde(default = "default_mode")] default_mode: String` (`"default"`). Unknown ids are kept as they are, since the frontend maps them to Default. This is required: `set_config` (`crates/app/src/commands.rs`) deserialises into the typed `Config` and re-saves it, so a field Rust doesn't model is erased on the next save of any setting.
- **Migration.** `Ui.compact` becomes `#[serde(default, skip_serializing)]`: it is read but never written. In `load_from`, after parsing, if `ui.compact` is true and `appearance.default_mode == "default"`, set `default_mode = "conversation"`. Conversation is Compact's closest successor, so a user who last left Compact on still gets a pared-down view. The next save drops `compact` from the file, so the migration fires at most once in effect, and a later explicit choice of Default sticks.
- **TS** (`lib/config.ts`): `Config` gains `appearance: { defaultMode: string }` (default `"default"`), and `ui.compact` is removed. `mergeConfig` in `hooks/useConfig.tsx` is shallow per section and gains an `appearance` line.
- No `modes` array, and no storage for custom modes.

## Copy as image

"Copy as image" (`lib/messageImage.ts` `captureTurn`) captures the turn's DOM as it is displayed. Hidden blocks and collapsed parts are absent, and a clamped body appears clamped, with its fade and its "show all N lines" button. The button is left in on purpose: it tells the reader the output continues. Only the existing `data-capture-skip` chrome is left out. Everything mode makes very tall turns likelier, and a turn over 32768 CSS px still fails with "Message too tall to capture" (`captureScale`), which is unchanged.

## Performance

- `MODES` entries are frozen module constants, and the `mode` passed down has a stable identity until the user switches.
- `visibleTurns = useMemo(() => applyMode(session.turns, mode), [mode, session.turns])`. Step 1 of `applyMode` returns `session.turns` itself for four of the five modes, and the filter keeps turn identities otherwise. `useFind`'s recompute keys on this array, so a fresh array per render would loop (the existing comment on `visibleTurns` says so).
- `Turn`, `BlockRenderer` and `TaskNotificationBlock` stay `memo`. They gain a `mode` prop (the constant) and nothing else per render: no inline objects, no new closures. `ToolCall` receives `mode` from `BlockRenderer` and derives its parts with `useMemo` on `(mode, block)`. A mode switch re-renders every turn once, and a find keystroke or live-tail append re-renders none that it didn't before.
- Open bodies mount their heavy children (`DiffBody`/`BashBody` highlighting, `AnsiText`, `FileDiff`) on mount, and a clamp does not reduce that cost. Truncated and Everything therefore mount every tool body. The builder measures switching to Everything on the largest local session in the agent app and reports the time. If it exceeds about 1 s, the permitted mitigation is to defer mounting open tool bodies until near the viewport. That is safe because tool bodies are model-sourced in find (`MODEL_FIELDS`), with `forceExpand` forcing the mount.

## Testing

- `lib/appearance.test.ts`:
  - `MODES` has five presets in cycle order with unique ids, and every preset has the same complete key set.
  - `modeById` maps an unknown id (including the retired `"skeleton"`) to Default, and `nextMode` wraps.
  - `applyMode` returns the same array for Default, Inputs, Truncated and Everything, over a fixture with every turn type and a thinking-only assistant turn.
  - Conversation drops `system`, `task_notification` and tool-only or narration-only assistant turns. It keeps text-bearing assistant turns, user, command, bash, summary, api_error, compaction and thinking-only turns, and kept turns are the same objects.
- Part resolution: an Edit input follows `diffs`, a Bash or JSON input follows `toolInput`, and file changes follow `diffs`.
- `components/ToolCall.test.tsx` (`renderToStaticMarkup`, as in `FileChanges.test.tsx`):
  - **Default golden.** Capture the current component's markup in the first commit, both collapsed and with `forceExpand`, for Bash, Edit, an `mcp__*` JSON result, an error result, no result, file changes and returned images. Default must then match it byte for byte.
  - Truncated: the `…:tool-result` element's text equals `toolResultText` in full, the clamp style is on its box, and the control is outside the seg element. `forceExpand` removes the clamp.
  - Everything: no `maxHeight` anywhere. Inputs: the input is rendered, and an output sub-row is rendered with no `tool-result` seg.
- `lib/findMatches.test.ts`: `modelMatches` with tool calls hidden returns no `tool-*` matches and still returns `task-result` matches. `revealKey` maps a `summary` match to `"<ti>:summary"`.
- `lib/jumpTarget.test.ts`: a uuid in a turn dropped by Conversation gives `mode-hidden`.
- `lib/windowView.test.ts`: seeds `modeId` from `appearance.defaultMode`, maps an unknown or retired id to `"default"`, ignores later config changes once seeded, and a `modeId` patch leaves `detailsOpen` alone.
- `config.rs`: `appearance` defaults to `"default"` when absent and round-trips. `ui.compact: true` with no `appearance` loads as `"conversation"`. `ui.compact: true` with `"inputs"` stays `"inputs"`. A saved config never contains `compact`.
- Gates: `cargo test`, `cargo clippy --workspace --all-targets -- -D warnings`, `bash scripts/tests/guided_test.sh`, `npm --prefix crates/app/frontend test`, and `npx tsc --noEmit`.
- Agent app (`verify-ui`):
  - Use a real session with long Bash output, an Edit, an `mcp__*` result, narration, a task notification and a compaction divider.
  - Screenshot Default before and after the change; the two must match.
  - Screenshot each mode.
  - In Truncated, find a word below a clamp and check that the clamp lifts and the highlight lands on the word.
  - Press ⌘⇧D to cycle modes.
  - Jump to a bookmark hidden in Conversation, and check the banner and its "Show in Default mode".
  - Open a second window and check it keeps its own mode.
  - Change the Settings default, open a new window and check it starts in that mode.
  - Check `scripts/ui logs` for errors.

## FEATURES.md

Replace the **Compact** toggle entry under "Browsing & viewing" with an **Appearance modes** entry. It lists the five modes, the header dropdown, ⌘⇧D, the per-window choice with Settings' default, visual clamping with "show all", and that find opens what it lands in, followed by `→ 2026-11-10-appearance-modes-design`. Reword the two other places that would otherwise be false. The API-errors entry's "Kept in compact mode" becomes "Shown in every appearance mode". In the Customization entry, "compact / details are per window, a new window starting from the last-used value" becomes: the appearance mode is per window and starts from Settings' default, and Details is per window and starts from its last-used value.

## Decisions

**The user's:**
- The mode is per window. Settings has one new control, "Default mode for new windows", and a window's choice does not survive a restart.
- Compact does not survive as a mode: the user tried a Skeleton preset (prompts plus the first and last turn of each reply) and removed it, since Conversation covers that use. The header gets one mode dropdown, and Details stays.
- The five presets and what each one means. Default must render exactly what the app renders today.
- Presets only: no editor, no custom modes, no per-tool rules and no storage for them. The presets are still expressed as one complete typed structure.

**The orchestrator's (builder's calls):**
- Clamping is visual (CSS line clamp, fade, "show all N lines"), never text slicing, and a find hit or jump into a collapsed or clamped part opens it.
- A jump to a mode-hidden message shows the miss banner with "Show in Default mode", which switches this window only.
- `appearance.defaultMode` lives in Rust and TS with no modes array, and `ui.compact: true` migrates to Conversation.
- `windowView`'s Compact flag becomes the mode id, and Details is unchanged.
- ⌘⇧D cycles modes (confirmed free).
- The dropdown reuses `Dropdown.tsx` and is accent-styled off Default.
- The FEATURES.md entry.

**This design's (builder's calls):**
- The `Height` type (`scroll` keeps today's per-kind boxes, so Default is a preset rather than a special case).
- No per-part `hidden` inside a shown tool call.
- Bash file changes follow `diffs`.
- Inputs keeps today's scroll boxes rather than going full height.
- The clamp line counts: 12, 15, 30, 15, 20 and 15.
- `api_error` and `!` shell turns show in every mode, and Conversation keeps the compaction summary.
- `AskUserQuestion`, as a tool call, is hidden in Conversation.
- Hidden blocks render `null` and are not removed from the turn.
- The header-chevron and sub-row semantics, and a mode switch resetting clicks.
- ⌘⇧D is ignored on the Terminal tab.
- The viewport is anchored across a switch.
- The "show all" control is kept in image captures.
- The migration uses `skip_serializing`.
