import { describe, it, expect } from "vitest";
import { createTranscriptLoader, type LoaderDeps } from "./transcriptLoader";
import type { LiveStats, RenderedSession, RenderedTurn } from "../types";

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((res) => (resolve = res));
  return { promise, resolve };
}

const settle = () => new Promise((r) => setTimeout(r, 0));
const session = (id: string) => ({ id, turns: [] }) as unknown as RenderedSession;
const turn = (text: string) => ({ type: "user", text }) as unknown as RenderedTurn;

/** A window's backend: `open_session` waits until the test resolves it; events reach every current listener. */
function fakeWindow() {
  // Registrations, not handlers: like Tauri, listening twice with the same callback delivers twice.
  const listeners = new Map<string, Set<{ handler: (p: unknown) => void }>>();
  const opens: { filePath: string; reply: ReturnType<typeof deferred<RenderedSession>> }[] = [];
  const deps: LoaderDeps = {
    invoke: (async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd !== "open_session") return undefined;
      const reply = deferred<RenderedSession>();
      opens.push({ filePath: String(args?.filePath), reply });
      return reply.promise;
    }) as LoaderDeps["invoke"],
    listen: (async (event: string, handler: (p: unknown) => void) => {
      const set = listeners.get(event) ?? new Set();
      const registration = { handler };
      set.add(registration);
      listeners.set(event, set);
      return () => set.delete(registration);
    }) as LoaderDeps["listen"],
  };
  const emit = (event: string, payload: unknown) => listeners.get(event)?.forEach((r) => r.handler(payload));
  const subscribers = (event: string) => listeners.get(event)?.size ?? 0;
  return { deps, opens, emit, subscribers };
}

function recordingSink() {
  const loaded: string[] = [];
  const appended: RenderedTurn[][] = [];
  const stats: LiveStats[] = [];
  return {
    loaded,
    appended,
    stats,
    sink: {
      loaded: (s: RenderedSession) => loaded.push(s.id),
      appended: (t: RenderedTurn[]) => appended.push(t),
      toolResults: () => {},
      retitled: () => {},
      stats: (u: LiveStats) => stats.push(u),
    },
  };
}

describe("createTranscriptLoader", () => {
  it("shows the opened session and appends its live turns once", async () => {
    const w = fakeWindow();
    const r = recordingSink();
    const loader = createTranscriptLoader(w.deps, r.sink);
    const opening = loader.open("/a.jsonl", "p");
    await settle();
    w.opens[0].reply.resolve(session("A"));
    expect(await opening).toBe("opened");
    w.emit("session-updated", [turn("hi")]);
    expect(r.loaded).toEqual(["A"]);
    expect(r.appended).toHaveLength(1);
  });

  it("keeps the header's figures current as the session grows, until closed", async () => {
    const w = fakeWindow();
    const r = recordingSink();
    const loader = createTranscriptLoader(w.deps, r.sink);
    const opening = loader.open("/a.jsonl", "p");
    await settle();
    w.opens[0].reply.resolve(session("A"));
    await opening;
    const update = { stats: { context_tokens: 143_147 }, event_count: 291 } as unknown as LiveStats;
    w.emit("session-stats", update);
    expect(r.stats).toEqual([update]);
    await loader.close();
    expect(w.subscribers("session-stats")).toBe(0);
  });

  it("keeps one subscription when a second open starts before the first finishes", async () => {
    const w = fakeWindow();
    const r = recordingSink();
    const loader = createTranscriptLoader(w.deps, r.sink);
    const first = loader.open("/big.jsonl", "p");
    await settle();
    const second = loader.open("/small.jsonl", "p");
    await settle();
    w.opens[0].reply.resolve(session("BIG"));
    w.opens[1].reply.resolve(session("SMALL"));
    const outcomes = [await first, await second];
    expect(w.subscribers("session-updated")).toBe(1);
    w.emit("session-updated", [turn("live")]);
    expect(r.appended).toHaveLength(1);
    expect(r.loaded).toEqual(["SMALL"]);
    expect(outcomes).toEqual(["superseded", "opened"]);
  });

  it("ignores an older open that finishes after the newer one", async () => {
    const w = fakeWindow();
    const r = recordingSink();
    const loader = createTranscriptLoader(w.deps, r.sink);
    const first = loader.open("/big.jsonl", "p");
    await settle();
    const second = loader.open("/small.jsonl", "p");
    await settle();
    w.opens[1].reply.resolve(session("SMALL"));
    await second;
    w.opens[0].reply.resolve(session("BIG"));
    const outcome = await first;
    expect(w.subscribers("session-updated")).toBe(1);
    expect(r.loaded).toEqual(["SMALL"]);
    expect(outcome).toBe("superseded");
  });

  it("subscribes to nothing when closed while an open is pending", async () => {
    const w = fakeWindow();
    const r = recordingSink();
    const loader = createTranscriptLoader(w.deps, r.sink);
    const opening = loader.open("/a.jsonl", "p");
    await settle();
    await loader.close();
    w.opens[0].reply.resolve(session("A"));
    const outcome = await opening;
    expect(w.subscribers("session-updated")).toBe(0);
    expect(r.loaded).toEqual([]);
    expect(outcome).toBe("superseded");
  });

  it("subscribes to nothing for a frozen branch view", async () => {
    const w = fakeWindow();
    const r = recordingSink();
    const loader = createTranscriptLoader(w.deps, r.sink);
    const opening = loader.open("/a.jsonl", "p", "some-uuid");
    await settle();
    w.opens[0].reply.resolve(session("A"));
    expect(await opening).toBe("opened");
    expect(w.subscribers("session-updated")).toBe(0);
  });
});
