import { describe, it, expect } from "vitest";
import { missingNotice, resolveLink, sessionLink } from "./sessionLink";
import type { SessionSummary } from "../types";

const ID = "0dd9d545-8a0a-44cc-a85b-23bb6896da97";
const session = (id: string) => ({ id, title: id }) as SessionSummary;

describe("sessionLink", () => {
  it("is the scheme the app registers, pointing at the session", () => {
    expect(sessionLink(ID)).toBe(`claude-code-manager://session/${ID}`);
  });
});

describe("resolveLink", () => {
  it("waits while the session list is loading", () => {
    expect(resolveLink(ID, [], true, false)).toEqual({ kind: "wait" });
  });

  it("opens a session that is in the list", () => {
    const s = session(ID);
    expect(resolveLink(ID, [session("other"), s], false, false)).toEqual({ kind: "open", session: s });
  });

  it("refreshes once before calling a session missing", () => {
    expect(resolveLink(ID, [session("other")], false, false)).toEqual({ kind: "refresh" });
    expect(resolveLink(ID, [session("other")], false, true)).toEqual({ kind: "missing" });
  });
});

describe("missingNotice", () => {
  it("tells a link apart from a fork: only a link can name a session that is gone", () => {
    expect(missingNotice("link")).toMatch(/deleted|another machine/);
    expect(missingNotice("fork")).toMatch(/fork was created/);
    expect(missingNotice("fork")).not.toMatch(/deleted/);
  });
});
