/** What is about to stop a session Claude is working in. */
export type StopKind = "close" | "quit" | "terminal" | "takeover" | "delete";

const RESUMABLE = "The session can be resumed, but the work in progress is lost.";

/** How the confirmation names the act — `<action> stops it mid-turn. <after>` over a `<confirm>` button. */
export function stopWording(kind: StopKind): { action: string; confirm: string; after: string } {
  switch (kind) {
    case "close":
      return { action: "Closing this window", confirm: "Close anyway", after: RESUMABLE };
    case "quit":
      return { action: "Quitting", confirm: "Quit anyway", after: RESUMABLE };
    case "terminal":
      return { action: "Closing its terminal", confirm: "Close anyway", after: RESUMABLE };
    case "takeover":
      return { action: "Opening it here", confirm: "Open here anyway", after: RESUMABLE };
    case "delete":
      return {
        action: "Deleting",
        confirm: "Delete anyway",
        after: "The session moves to the trash, and the work in progress is lost.",
      };
  }
}
