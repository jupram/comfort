import { describe, expect, it, vi } from "vitest";
import { GestureActionQueue } from "../src/gesture-action-queue.js";

describe("GestureActionQueue", () => {
  it("serializes delivery and coalesces queued scroll movement", async () => {
    let releaseFirst;
    const firstDelivery = new Promise((resolve) => { releaseFirst = resolve; });
    const send = vi.fn()
      .mockReturnValueOnce(firstDelivery)
      .mockResolvedValue(undefined);
    const queue = new GestureActionQueue(send);

    queue.enqueue({ type: "SCROLL", delta: 10 });
    queue.enqueue({ type: "SCROLL", delta: 12 });
    queue.enqueue({ type: "SCROLL", delta: 8 });

    expect(send).toHaveBeenCalledTimes(1);
    releaseFirst();
    await queue.whenIdle();

    expect(send).toHaveBeenCalledTimes(2);
    expect(send).toHaveBeenNthCalledWith(2, { type: "SCROLL", delta: 20 });
  });

  it("preserves click ordering between scroll batches", async () => {
    const send = vi.fn().mockResolvedValue(undefined);
    const queue = new GestureActionQueue(send);

    queue.enqueue({ type: "SCROLL", delta: 5 });
    queue.enqueue({ type: "SINGLE_CLICK" });
    queue.enqueue({ type: "SCROLL", delta: -7 });
    await queue.whenIdle();

    expect(send.mock.calls.map(([action]) => action)).toEqual([
      { type: "SCROLL", delta: 5 },
      { type: "SINGLE_CLICK" },
      { type: "SCROLL", delta: -7 },
    ]);
  });

  it("drops queued actions after a delivery failure", async () => {
    const onError = vi.fn();
    const send = vi.fn().mockRejectedValue(new Error("disconnected"));
    const queue = new GestureActionQueue(send, onError);

    queue.enqueue({ type: "SCROLL", delta: 5 });
    queue.enqueue({ type: "BROWSER_BACK" });
    await queue.whenIdle();

    expect(send).toHaveBeenCalledTimes(1);
    expect(onError).toHaveBeenCalledWith(expect.objectContaining({ message: "disconnected" }));
  });

  it("ignores stale failures after the queue is cleared for a new session", async () => {
    let rejectOldDelivery;
    const oldDelivery = new Promise((_resolve, reject) => { rejectOldDelivery = reject; });
    const send = vi.fn()
      .mockReturnValueOnce(oldDelivery)
      .mockResolvedValue(undefined);
    const onError = vi.fn();
    const queue = new GestureActionQueue(send, onError);

    queue.enqueue({ type: "SCROLL", delta: 5 });
    queue.clear();
    queue.enqueue({ type: "SINGLE_CLICK" });
    rejectOldDelivery(new Error("old session disconnected"));
    await queue.whenIdle();

    expect(onError).not.toHaveBeenCalled();
    expect(send).toHaveBeenLastCalledWith({ type: "SINGLE_CLICK" });
  });
});
