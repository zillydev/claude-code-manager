import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { unlistenWhenRegistered } from "./unlisten";

describe("unlistenWhenRegistered", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("calls once when the listener is already registered", async () => {
    const un = vi.fn().mockResolvedValue(undefined);
    await unlistenWhenRegistered(un);
    expect(un).toHaveBeenCalledTimes(1);
  });

  it("retries until the registration has landed", async () => {
    const un = vi
      .fn()
      .mockRejectedValueOnce(new TypeError("not registered yet"))
      .mockRejectedValueOnce(new TypeError("not registered yet"))
      .mockResolvedValue(undefined);
    const done = unlistenWhenRegistered(un);
    await vi.runAllTimersAsync();
    await done;
    expect(un).toHaveBeenCalledTimes(3);
  });

  it("gives up quietly on a listener that never registers", async () => {
    const un = vi.fn().mockRejectedValue(new TypeError("gone"));
    const done = unlistenWhenRegistered(un);
    await vi.runAllTimersAsync();
    await expect(done).resolves.toBeUndefined();
    expect(un).toHaveBeenCalledTimes(6);
  });

  it("treats a synchronous throw like a rejection", async () => {
    const un = vi.fn().mockImplementationOnce(() => {
      throw new TypeError("sync");
    }).mockResolvedValue(undefined);
    const done = unlistenWhenRegistered(un as () => Promise<void>);
    await vi.runAllTimersAsync();
    await done;
    expect(un).toHaveBeenCalledTimes(2);
  });
});
