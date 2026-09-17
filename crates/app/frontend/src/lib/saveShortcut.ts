type KeyFacts = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey" | "isComposing">;

/** ⌘↵ alone, the macOS "submit this text" chord; not while an input method is composing. */
export function isSaveShortcut(e: KeyFacts): boolean {
  return e.key === "Enter" && e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey && !e.isComposing;
}
