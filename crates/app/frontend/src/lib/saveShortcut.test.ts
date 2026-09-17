import { describe, it, expect } from "vitest";
import { isSaveShortcut } from "./saveShortcut";

const key = (over: Partial<KeyboardEvent>) => ({
  key: "Enter",
  metaKey: false,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  isComposing: false,
  ...over,
});

describe("isSaveShortcut", () => {
  it("is ⌘↵", () => {
    expect(isSaveShortcut(key({ metaKey: true }))).toBe(true);
  });

  it("isn't a plain ↵, which types a newline", () => {
    expect(isSaveShortcut(key({}))).toBe(false);
  });

  it("isn't ⌘ with another modifier, or ⌃↵", () => {
    expect(isSaveShortcut(key({ metaKey: true, shiftKey: true }))).toBe(false);
    expect(isSaveShortcut(key({ metaKey: true, altKey: true }))).toBe(false);
    expect(isSaveShortcut(key({ ctrlKey: true }))).toBe(false);
  });

  it("isn't ⌘ with another key", () => {
    expect(isSaveShortcut(key({ key: "s", metaKey: true }))).toBe(false);
  });

  it("waits while an input method is composing", () => {
    expect(isSaveShortcut(key({ metaKey: true, isComposing: true }))).toBe(false);
  });
});
