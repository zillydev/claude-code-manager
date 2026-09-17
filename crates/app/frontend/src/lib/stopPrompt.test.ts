import { describe, it, expect } from "vitest";
import { stopWording } from "./stopPrompt";

describe("stopWording", () => {
  it("names the act that stops the session", () => {
    expect(stopWording("close").action).toBe("Closing this window");
    expect(stopWording("quit").action).toBe("Quitting");
    expect(stopWording("terminal").action).toBe("Closing its terminal");
    expect(stopWording("takeover").action).toBe("Opening it here");
  });

  it("confirms with the act, not a generic close", () => {
    expect(stopWording("quit").confirm).toBe("Quit anyway");
    expect(stopWording("takeover").confirm).toBe("Open here anyway");
    expect(stopWording("close").confirm).toBe("Close anyway");
    expect(stopWording("terminal").confirm).toBe("Close anyway");
  });
});
