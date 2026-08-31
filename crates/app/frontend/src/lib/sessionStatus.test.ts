import { describe, it, expect } from "vitest";
import { deriveStatus, showsUnread } from "./sessionStatus";

describe("deriveStatus", () => {
  it("running while mid-turn (not awaiting) and not stale — through thinking pauses", () => {
    // 60s since the last write but still mid-turn → still running (no flicker).
    expect(deriveStatus(1000, false, 0, 1060)).toEqual({ running: true, unread: false });
  });
  it("not running once the turn completes (awaiting), even seconds after a write", () => {
    expect(deriveStatus(1000, true, 0, 1001).running).toBe(false);
  });
  it("not running when a mid-turn session has gone stale (abandoned)", () => {
    expect(deriveStatus(1000, false, 0, 1000 + 181).running).toBe(false);
  });
  it("unread only when a completed turn is newer than last viewed", () => {
    expect(deriveStatus(1000, true, 999, 9999).unread).toBe(true);
    expect(deriveStatus(1000, true, 1000, 9999).unread).toBe(false);
  });
  it("never unread while mid-turn, however far it has advanced", () => {
    expect(deriveStatus(1000, false, 0, 9999).unread).toBe(false);
  });
  it("a busy terminal keeps running however long a tool call runs", () => {
    // The .jsonl is silent during a build, but claude's TUI keeps redrawing.
    const busy = { busy: true, busyAt: 1000 + 3599 };
    expect(deriveStatus(1000, false, 0, 1000 + 3600, busy).running).toBe(true);
    // …but a finished turn still ends "running" at once, busy or not.
    expect(deriveStatus(1000, true, 0, 1001, busy).running).toBe(false);
  });

  it("a hosted session whose terminal has gone quiet is not running", () => {
    // Parked on a question: the process is alive but silent, and Claude Code
    // writes nothing until it is answered. The terminal outranks the mtime
    // guess here — even seconds after the prompt was written.
    const quiet = { busy: false, busyAt: 1002 };
    expect(deriveStatus(1000, false, 0, 1000 + 900, quiet).running).toBe(false);
    expect(deriveStatus(1000, false, 0, 1000 + 5, quiet).running).toBe(false);
    // A session ccm does NOT host keeps the guess, since nothing better exists.
    expect(deriveStatus(1000, false, 0, 1000 + 5, null).running).toBe(true);
  });

  it("a session parked on a question is unread, dated from when it went quiet", () => {
    // You prompted at 1000 (so you have seen that far), claude worked until
    // 1050 and has been sitting on a question since. Nothing is written to the
    // file until you answer, so only the terminal can date this.
    const parked = { busy: false, busyAt: 1050 };
    expect(deriveStatus(1000, false, 1000, 1100, parked)).toEqual({
      running: false,
      unread: true,
    });
    // Looked at it since it fell quiet → seen.
    expect(deriveStatus(1000, false, 1060, 1100, parked).unread).toBe(false);
    // Still working → your turn hasn't come.
    expect(deriveStatus(1000, false, 1000, 1100, { busy: true, busyAt: 1099 }).unread).toBe(false);
    // Not hosted: ccm can't see a question, so it can't claim one.
    expect(deriveStatus(1000, false, 1000, 1100, null).unread).toBe(false);
  });

  it("never both running and unread", () => {
    const cases = [
      [false, { busy: true, busyAt: 1099 }],
      [false, { busy: false, busyAt: 1050 }],
      [true, { busy: true, busyAt: 1099 }],
      [true, null],
      [false, null],
    ] as const;
    for (const [awaiting, terminal] of cases) {
      const s = deriveStatus(1000, awaiting, 0, 1100, terminal);
      expect(s.running && s.unread).toBe(false);
    }
  });
  it("neither when there are no events", () => {
    expect(deriveStatus(null, false, 0, 9999)).toEqual({ running: false, unread: false });
  });
});

describe("showsUnread", () => {
  const waiting = { running: false, unread: true } as const;
  it("stays quiet on the session you are looking at", () => {
    expect(showsUnread(waiting, true, true)).toBe(false);
  });
  it("still nags when its window is behind another or on another Space", () => {
    expect(showsUnread(waiting, true, false)).toBe(true);
  });
  it("leaves other sessions alone either way", () => {
    expect(showsUnread(waiting, false, true)).toBe(true);
    expect(showsUnread({ running: true, unread: false }, false, false)).toBe(false);
  });
});

describe("hook phases", () => {
  it("attention means unread the moment the question appears — repaints be damned", () => {
    // Terminal is busy (the user is browsing the options); hooks know better.
    const busy = { busy: true, busyAt: 1090 };
    const s = deriveStatus(1000, false, 1000, 1100, busy, { phase: "attention", since: 1050 });
    expect(s).toEqual({ running: false, unread: true });
  });
  it("running through a silent tool call, no volume needed", () => {
    const quiet = { busy: false, busyAt: 1010 };
    expect(deriveStatus(1000, false, 0, 1100, quiet, { phase: "running", since: 1001 }).running).toBe(true);
  });
  it("awaiting goes unread instantly, and viewing clears it", () => {
    expect(deriveStatus(1000, false, 1000, 1004, { busy: false, busyAt: 1002 }, { phase: "awaiting", since: 1003 }).unread).toBe(true);
    expect(deriveStatus(1000, false, 1005, 1010, { busy: false, busyAt: 1002 }, { phase: "awaiting", since: 1003 }).unread).toBe(false);
  });
  it("a hook that has not seen the file's newest event defers to inference", () => {
    // An interrupt wrote a user line at 1080; the hook still thinks a question
    // from 1050 is up. The file wins: fresh + not awaiting ⇒ the mtime guess.
    expect(deriveStatus(1080, false, 0, 1085, null, { phase: "attention", since: 1050 })).toEqual({
      running: true,
      unread: false,
    });
  });
  it("unhosted hook-running decays so a SIGKILLed session cannot strand", () => {
    expect(deriveStatus(1000, false, 0, 1100, null, { phase: "running", since: 1001 }).running).toBe(true);
    expect(deriveStatus(1000, false, 0, 1300, null, { phase: "running", since: 1001 }).running).toBe(false);
  });
});
