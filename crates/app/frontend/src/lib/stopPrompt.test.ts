import { describe, it, expect } from "vitest";
import { stopWording } from "./stopPrompt";

describe("stopWording", () => {
  it("names the act that stops the session", () => {
    expect(stopWording("close").action).toBe("Closing this window");
    expect(stopWording("quit").action).toBe("Quitting");
    expect(stopWording("terminal").action).toBe("Closing its terminal");
    expect(stopWording("takeover").action).toBe("Opening it here");
    expect(stopWording("delete").action).toBe("Deleting");
  });

  it("confirms with the act, not a generic close", () => {
    expect(stopWording("quit").confirm).toBe("Quit anyway");
    expect(stopWording("takeover").confirm).toBe("Open here anyway");
    expect(stopWording("close").confirm).toBe("Close anyway");
    expect(stopWording("terminal").confirm).toBe("Close anyway");
    expect(stopWording("delete").confirm).toBe("Delete anyway");
  });

  it("says what becomes of the session afterwards", () => {
    expect(stopWording("close").after).toBe("The session can be resumed, but the work in progress is lost.");
    expect(stopWording("delete").after).toBe("The session moves to the trash, and the work in progress is lost.");
  });
});
