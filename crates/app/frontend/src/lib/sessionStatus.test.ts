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
    const busy = { busy: true, busyAt: 1000 + 3599, interruptAt: 0 };
    expect(deriveStatus(1000, false, 0, 1000 + 3600, busy).running).toBe(true);
    // …but a finished turn still ends "running" at once, busy or not.
    expect(deriveStatus(1000, true, 0, 1001, busy).running).toBe(false);
  });

  it("a hosted session whose terminal has gone quiet is not running", () => {
    // Parked on a question: the process is alive but silent, and Claude Code
    // writes nothing until it is answered. The terminal outranks the mtime
    // guess here — even seconds after the prompt was written.
    const quiet = { busy: false, busyAt: 1002, interruptAt: 0 };
    expect(deriveStatus(1000, false, 0, 1000 + 900, quiet).running).toBe(false);
    expect(deriveStatus(1000, false, 0, 1000 + 5, quiet).running).toBe(false);
    // A session ccm does NOT host keeps the guess, since nothing better exists.
    expect(deriveStatus(1000, false, 0, 1000 + 5, null).running).toBe(true);
  });

  it("a session parked on a question is unread, dated from when it went quiet", () => {
    // You prompted at 1000 (so you have seen that far), claude worked until
    // 1050 and has been sitting on a question since. Nothing is written to the
    // file until you answer, so only the terminal can date this.
    const parked = { busy: false, busyAt: 1050, interruptAt: 0 };
    expect(deriveStatus(1000, false, 1000, 1100, parked)).toEqual({
      running: false,
      unread: true,
    });
    // Looked at it since it fell quiet → seen.
    expect(deriveStatus(1000, false, 1060, 1100, parked).unread).toBe(false);
    // Still working → your turn hasn't come.
    expect(deriveStatus(1000, false, 1000, 1100, { busy: true, busyAt: 1099, interruptAt: 0 }).unread).toBe(false);
    // Not hosted: ccm can't see a question, so it can't claim one.
    expect(deriveStatus(1000, false, 1000, 1100, null).unread).toBe(false);
  });

  it("never both running and unread", () => {
    const cases = [
      [false, { busy: true, busyAt: 1099, interruptAt: 0 }],
      [false, { busy: false, busyAt: 1050, interruptAt: 0 }],
      [true, { busy: true, busyAt: 1099, interruptAt: 0 }],
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
    const busy = { busy: true, busyAt: 1090, interruptAt: 0 };
    const s = deriveStatus(1000, false, 1000, 1100, busy, { phase: "attention", since: 1050 });
    expect(s).toEqual({ running: false, unread: true });
  });
  it("running through a silent tool call, no volume needed", () => {
    const quiet = { busy: false, busyAt: 1010, interruptAt: 0 };
    expect(deriveStatus(1000, false, 0, 1100, quiet, { phase: "running", since: 1001 }).running).toBe(true);
  });
  it("awaiting goes unread instantly, and viewing clears it", () => {
    expect(deriveStatus(1000, false, 1000, 1004, { busy: false, busyAt: 1002, interruptAt: 0 }, { phase: "awaiting", since: 1003 }).unread).toBe(true);
    expect(deriveStatus(1000, false, 1005, 1010, { busy: false, busyAt: 1002, interruptAt: 0 }, { phase: "awaiting", since: 1003 }).unread).toBe(false);
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

describe("hook running through a turn", () => {
  it("is not handed back to the quiet-terminal guess by tool lines", () => {
    // Prompt at 1000 (hook running), tool lines up to 1200, terminal silent
    // since 1160 (a long API wait). Inference would read that as parked.
    const quiet = { busy: false, busyAt: 1160, interruptAt: 0 };
    expect(deriveStatus(1200, false, 900, 1205, quiet, { phase: "running", since: 1000 })).toEqual({
      running: true,
      unread: false,
    });
  });
  it("is refuted by a finished turn or an interrupt in the file", () => {
    const quiet = { busy: false, busyAt: 1160, interruptAt: 0 };
    // awaitingInput covers end_turn, [Request interrupted], and API errors.
    expect(deriveStatus(1200, true, 900, 1205, quiet, { phase: "running", since: 1000 })).toEqual({
      running: false,
      unread: true,
    });
  });
  it("unhosted, decays from the last sign of life rather than the prompt", () => {
    const h = { phase: "running" as const, since: 1000 };
    expect(deriveStatus(1500, false, 0, 1600, null, h).running).toBe(true);   // file still moving
    expect(deriveStatus(1500, false, 0, 1700, null, h).running).toBe(false);  // 200s of nothing
  });
});

describe("an Esc in ccm's own terminal", () => {
  it("ends a turn the file and hooks both missed", () => {
    // Prompt at 1000, Esc at 1001 before any token: no reply line, no
    // interrupt marker, hook still says running, PTY alive and quiet.
    const t = { busy: false, busyAt: 1001, interruptAt: 1001 };
    const h = { phase: "running" as const, since: 1000 };
    expect(deriveStatus(1000, false, 900, 1100, t, h)).toEqual({ running: false, unread: true });
    // You pressed it, so you've seen it: viewing at the Esc clears it.
    expect(deriveStatus(1000, false, 1001, 1100, t, h).unread).toBe(false);
  });
  it("changes nothing at an idle prompt, or when the file is newer", () => {
    const h = { phase: "awaiting" as const, since: 1000 };
    expect(deriveStatus(1000, true, 1000, 1100, { busy: false, busyAt: 999, interruptAt: 1050 }, h).unread).toBe(false);
    // Claude answered after the Esc after all: the file's newer line wins.
    expect(deriveStatus(1080, false, 900, 1085, { busy: true, busyAt: 1084, interruptAt: 1050 }, null).running).toBe(true);
  });
});
