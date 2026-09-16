import { describe, it, expect } from "vitest";
import { statusInputs, workingSessions } from "./closeGuard";
import type { LiveActivity } from "../hooks/useActivity";
import type { SessionSummary } from "../types";

const NOW = 1_000_000;
const session = (id: string, awaiting = false): SessionSummary =>
  ({ id, title: id, last_event_at: { secs_since_epoch: NOW - 5, nanos_since_epoch: 0 }, awaiting_input: awaiting }) as SessionSummary;
const act = (over: Partial<LiveActivity>): LiveActivity => ({
  at: NOW - 5, awaiting: false, hosted: true, owner: "main", busy: false, busyAt: NOW - 5, hook: null, interruptAt: 0, ...over,
});

describe("statusInputs", () => {
  it("prefers live activity over the summary, and passes the terminal only when hosted", () => {
    const s = session("a");
    expect(statusInputs(s, act({ at: NOW, busy: true }))).toEqual({
      lastEventAt: NOW, awaiting: false, terminal: { busy: true, busyAt: NOW - 5, interruptAt: 0 }, hook: null,
    });
    expect(statusInputs(s, act({ hosted: false })).terminal).toBeNull();
    expect(statusInputs(s, undefined)).toEqual({ lastEventAt: NOW - 5, awaiting: false, terminal: null, hook: null });
  });
});

describe("workingSessions", () => {
  const sessions = [session("working"), session("idle"), session("done", true), session("unhosted")];
  const activity = new Map<string, LiveActivity>([
    ["working", act({ busy: true })],
    ["idle", act({ busy: false })],
    ["done", act({ busy: false, awaiting: true })],
  ]);

  it("returns the given sessions Claude is working in, as the sidebar's running dot says", () => {
    expect(workingSessions(["working", "idle", "done"], sessions, activity, NOW).map((s) => s.id)).toEqual(["working"]);
  });

  it("ignores ids outside the given set, and ids with no session", () => {
    expect(workingSessions(["idle"], sessions, activity, NOW)).toEqual([]);
    expect(workingSessions(["gone"], sessions, activity, NOW)).toEqual([]);
  });
});
