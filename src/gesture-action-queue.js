const MAX_ABSOLUTE_SCROLL_DELTA = 240;

export class GestureActionQueue {
  constructor(sendAction, onError = () => {}) {
    if (typeof sendAction !== "function") throw new TypeError("sendAction must be a function");
    this.sendAction = sendAction;
    this.onError = onError;
    this.queue = [];
    this.drainPromise = null;
    this.generation = 0;
  }

  enqueue(action) {
    if (!action || typeof action.type !== "string") return this.whenIdle();

    const normalized = normalizeAction(action);
    const tail = this.queue.at(-1);
    if (normalized.type === "SCROLL" && tail?.action.type === "SCROLL") {
      tail.action.delta = clamp(
        tail.action.delta + normalized.delta,
        -MAX_ABSOLUTE_SCROLL_DELTA,
        MAX_ABSOLUTE_SCROLL_DELTA,
      );
    } else {
      this.queue.push({ action: normalized, generation: this.generation });
    }

    if (!this.drainPromise) {
      this.drainPromise = this.drain().finally(() => {
        this.drainPromise = null;
        if (this.queue.length > 0) void this.enqueueDrain();
      });
    }
    return this.drainPromise;
  }

  clear() {
    this.generation += 1;
    this.queue.length = 0;
  }

  whenIdle() {
    return this.drainPromise ?? Promise.resolve();
  }

  async enqueueDrain() {
    if (this.drainPromise || this.queue.length === 0) return;
    this.drainPromise = this.drain().finally(() => {
      this.drainPromise = null;
      if (this.queue.length > 0) void this.enqueueDrain();
    });
    await this.drainPromise;
  }

  async drain() {
    while (this.queue.length > 0) {
      const { action, generation } = this.queue.shift();
      try {
        await this.sendAction(action);
      } catch (error) {
        if (generation !== this.generation) continue;
        this.clear();
        await this.onError(error);
        return;
      }
    }
  }
}

function normalizeAction(action) {
  if (action.type !== "SCROLL") return { type: action.type };
  return {
    type: "SCROLL",
    delta: clamp(Number(action.delta) || 0, -MAX_ABSOLUTE_SCROLL_DELTA, MAX_ABSOLUTE_SCROLL_DELTA),
  };
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}
