/** What is about to stop a session Claude is working in. */
export type StopKind = "close" | "quit" | "terminal" | "takeover";

/** How the confirmation names the act: `<action> stops it mid-turn`, over a `<confirm>` button. */
export function stopWording(kind: StopKind): { action: string; confirm: string } {
  switch (kind) {
    case "close":
      return { action: "Closing this window", confirm: "Close anyway" };
    case "quit":
      return { action: "Quitting", confirm: "Quit anyway" };
    case "terminal":
      return { action: "Closing its terminal", confirm: "Close anyway" };
    case "takeover":
      return { action: "Opening it here", confirm: "Open here anyway" };
  }
}
