import { describe, it, expect } from "vitest";
import { openBadge, openSessions, type OpenSession } from "./openSessions";
import type { LiveActivity } from "../hooks/useActivity";
import type { SessionSummary } from "../types";

const NOW = 1_000_000;
const session = (id: string, updated: number) =>
  ({
    id,
    title: id,
    last_event_at: { secs_since_epoch: updated, nanos_since_epoch: 0 },
    mtime: { secs_since_epoch: updated, nanos_since_epoch: 0 },
    awaiting_input: false,
  }) as SessionSummary;
const act = (over: Partial<LiveActivity>): LiveActivity => ({
  at: NOW - 60, awaiting: false, hosted: true, owner: "main", busy: false, busyAt: NOW - 60, hook: null, interruptAt: 0, ...over,
});

describe("openSessions", () => {
  const sessions = [session("closed", NOW - 1), session("idle-old", NOW - 500), session("idle-new", NOW - 10), session("working", NOW - 900)];
  const activity = new Map<string, LiveActivity>([
    ["closed", act({ hosted: false, owner: null, at: NOW - 1 })],
    ["idle-old", act({ at: NOW - 500, owner: "win-1" })],
    ["idle-new", act({ at: NOW - 10 })],
    ["working", act({ at: NOW - 900, busy: true })],
  ]);

  it("lists only sessions with a live terminal", () => {
    expect(openSessions(sessions, activity, NOW).map((o) => o.session.id)).not.toContain("closed");
  });

  it("puts sessions Claude is working in first, then the most recently updated", () => {
    expect(openSessions(sessions, activity, NOW).map((o) => o.session.id)).toEqual(["working", "idle-new", "idle-old"]);
  });

  it("marks which are working and which window runs each", () => {
    const rows = openSessions(sessions, activity, NOW);
    expect(rows.map((o) => [o.session.id, o.working, o.owner])).toEqual([
      ["working", true, "main"],
      ["idle-new", false, "main"],
      ["idle-old", false, "win-1"],
    ]);
  });
});

describe("openBadge", () => {
  const row = (id: string, working: boolean): OpenSession => ({ session: session(id, NOW), working, owner: "main" });

  it("shows nothing when no terminal is open", () => {
    expect(openBadge([])).toBeNull();
  });

  it("counts open sessions and stays neutral while all are idle", () => {
    expect(openBadge([row("a", false), row("b", false)])).toEqual({ label: "2", count: 2, working: false });
  });

  it("turns working when Claude works in any of them", () => {
    expect(openBadge([row("a", false), row("b", true)])).toEqual({ label: "2", count: 2, working: true });
  });

  it("caps the label at 99+", () => {
    const many = Array.from({ length: 120 }, (_, i) => row(`s${i}`, false));
    expect(openBadge(many)).toEqual({ label: "99+", count: 120, working: false });
  });
});
