import type { RenderedSession, RenderedTurn } from "../types";
import type { ToolResultUpdate } from "./toolResults";

type Unlisten = () => void;

export interface LoaderDeps {
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
  /** Subscribe to an event sent to this window; resolves once listening. */
  listen: <T>(event: string, handler: (payload: T) => void) => Promise<Unlisten>;
}

export interface LoaderSink {
  loaded: (session: RenderedSession) => void;
  appended: (turns: RenderedTurn[]) => void;
  toolResults: (updates: ToolResultUpdate[]) => void;
  retitled: (title: string) => void;
}

/** Opens sessions in one window and subscribes to the live tail of the one it shows. Opens are numbered: one
 *  overtaken by a newer open or a close shows nothing and subscribes to nothing, so a slow session parse that
 *  finishes late can't leave a second subscription appending every turn twice. */
export function createTranscriptLoader(deps: LoaderDeps, sink: LoaderSink) {
  let unsubscribe: Unlisten | null = null;
  let latest = 0;

  const dropSubscription = () => {
    unsubscribe?.();
    unsubscribe = null;
  };

  const open = async (filePath: string, projectLabel: string, branchUuid?: string): Promise<"opened" | "superseded"> => {
    const mine = ++latest;
    const stale = () => mine !== latest;
    dropSubscription();
    await deps.invoke("stop_watching").catch(() => {});
    let session: RenderedSession;
    try {
      session = await deps.invoke<RenderedSession>("open_session", { filePath, projectLabel, branchUuid });
    } catch (e) {
      if (stale()) return "superseded";
      throw e;
    }
    if (stale()) return "superseded";
    sink.loaded(session);
    if (branchUuid) return "opened";

    const offs: Unlisten[] = [];
    const subscriptions: [string, (payload: never) => void][] = [
      ["session-updated", sink.appended],
      ["session-tool-results", sink.toolResults],
      ["session-title", sink.retitled],
    ];
    for (const [event, handler] of subscriptions) {
      offs.push(await deps.listen(event, handler as (payload: unknown) => void));
      if (stale()) {
        offs.forEach((off) => off());
        return "superseded";
      }
    }
    unsubscribe = () => offs.forEach((off) => off());
    return "opened";
  };

  const close = async () => {
    latest++;
    dropSubscription();
    await deps.invoke("stop_watching").catch(() => {});
  };

  return { open, close };
}
