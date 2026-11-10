const RETRY_MS = 50;
const ATTEMPTS = 6;

/** Remove a Tauri listener, retrying while its page-side registration is still in flight.
 *  Tauri's unlisten throws before it reaches the backend in that window, which would leak the listener. */
export async function unlistenWhenRegistered(unlisten: () => void | Promise<void>): Promise<void> {
  for (let attempt = 0; attempt < ATTEMPTS; attempt++) {
    try {
      await unlisten();
      return;
    } catch {
      await new Promise((r) => setTimeout(r, RETRY_MS));
    }
  }
}
