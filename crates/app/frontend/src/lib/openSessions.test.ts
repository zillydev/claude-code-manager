import { describe, it, expect } from "vitest";
import { heldPending, openBadge, openSessions, type OpenSession } from "./openSessions";
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
  at: NOW - 60, awaiting: false, hosted: true, owner: "main", busy: false, busyAt: NOW - 60, hook: null, interruptAt: 0, program: null, pending: null, ...over,
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
    expect(openSessions(sessions, activity, NOW, "main").map((o) => o.session.id)).not.toContain("closed");
  });

  it("puts sessions Claude is working in first, then the most recently updated", () => {
    expect(openSessions(sessions, activity, NOW, "main").map((o) => o.session.id)).toEqual(["working", "idle-new", "idle-old"]);
  });

  it("marks which are working, which window runs each and whether it is this one", () => {
    const rows = openSessions(sessions, activity, NOW, "main");
    expect(rows.map((o) => [o.session.id, o.working, o.owner, o.mine])).toEqual([
      ["working", true, "main", true],
      ["idle-new", false, "main", true],
      ["idle-old", false, "win-1", false],
    ]);
  });

  it("puts this window's sessions first, each group working first, then most recent", () => {
    const ss = [session("a-idle", NOW - 5), session("a-work", NOW - 900), session("b-work", NOW - 1), session("b-idle", NOW - 400), session("a-old", NOW - 800)];
    const acts = new Map<string, LiveActivity>([
      ["a-idle", act({ at: NOW - 5, owner: "main" })],
      ["a-work", act({ at: NOW - 900, owner: "main", busy: true })],
      ["b-work", act({ at: NOW - 1, owner: "win-1", busy: true })],
      ["b-idle", act({ at: NOW - 400, owner: "win-1" })],
      ["a-old", act({ at: NOW - 800, owner: "main" })],
    ]);
    expect(openSessions(ss, acts, NOW, "main").map((o) => o.session.id)).toEqual(["a-work", "a-idle", "a-old", "b-work", "b-idle"]);
    expect(openSessions(ss, acts, NOW, "win-1").map((o) => o.session.id)).toEqual(["b-work", "b-idle", "a-work", "a-idle", "a-old"]);
  });

  it("treats a session with no owner yet as this window's", () => {
    const ss = [session("unowned", NOW - 600), session("other", NOW - 1)];
    const acts = new Map<string, LiveActivity>([
      ["unowned", act({ at: NOW - 600, owner: null })],
      ["other", act({ at: NOW - 1, owner: "win-1" })],
    ]);
    expect(openSessions(ss, acts, NOW, "main").map((o) => [o.session.id, o.mine])).toEqual([
      ["unowned", true],
      ["other", false],
    ]);
  });
});

describe("sessions with no conversation yet", () => {
  const fresh = { cwd: "/x/mono", filePath: "/p/fresh.jsonl", projectLabel: "mono" };
  const pendingAct = (over: Partial<LiveActivity> = {}) => act({ at: NOW - 5, awaiting: true, pending: fresh, ...over });

  it("lists a hosted session that has no file yet", () => {
    const rows = openSessions([], new Map([["fresh", pendingAct()]]), NOW, "main");
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ pending: true, working: false, mine: true });
    expect(rows[0].session).toMatchObject({ id: "fresh", title: "New session", project_label: "mono", file_path: "/p/fresh.jsonl", cwd: "/x/mono" });
  });

  it("yields to its discovered summary", () => {
    const rows = openSessions([session("fresh", NOW - 5)], new Map([["fresh", pendingAct()]]), NOW, "main");
    expect(rows.map((r) => [r.session.id, r.pending, r.session.title])).toEqual([["fresh", false, "fresh"]]);
  });

  it("is not listed once its terminal closes", () => {
    expect(openSessions([], new Map([["fresh", pendingAct({ hosted: false })]]), NOW, "main")).toEqual([]);
  });

  it("sorts by spawn time among idle rows", () => {
    const acts = new Map([["fresh", pendingAct()], ["idle-new", act({ at: NOW - 10 })]]);
    expect(openSessions([session("idle-new", NOW - 10)], acts, NOW, "main").map((r) => r.session.id)).toEqual(["fresh", "idle-new"]);
  });

  it("counts toward the badge", () => {
    expect(openBadge(openSessions([], new Map([["fresh", pendingAct()]]), NOW, "main"))?.count).toBe(1);
  });

  it("keeps its row when the poll that drops `pending` beats the refetched session list", () => {
    const first = heldPending(new Map(), [], new Map([["fresh", pendingAct()]]));
    // The file now exists: the next poll's row is the scanned one, but `sessions` hasn't caught up.
    const scanned = new Map([["fresh", act({ at: NOW - 1 })]]);
    const held = heldPending(first, [], scanned);
    expect(openSessions([], scanned, NOW, "main", held).map((r) => [r.session.id, r.pending])).toEqual([["fresh", true]]);
    // Once the summary arrives, the placeholder is released.
    expect(heldPending(held, [session("fresh", NOW - 1)], scanned).size).toBe(0);
  });

  it("drops a held placeholder whose terminal closed", () => {
    const first = heldPending(new Map(), [], new Map([["fresh", pendingAct()]]));
    expect(heldPending(first, [], new Map([["fresh", act({ hosted: false })]])).size).toBe(0);
    expect(heldPending(first, [], new Map()).size).toBe(0);
  });
});

describe("openBadge", () => {
  const row = (id: string, working: boolean): OpenSession => ({ session: session(id, NOW), working, owner: "main", mine: true, pending: false });

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
