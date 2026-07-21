import { afterEach, describe, expect, it, vi } from "vitest";

describe("background content-script recovery", () => {
  afterEach(() => {
    delete globalThis.chrome;
    vi.restoreAllMocks();
    vi.resetModules();
  });

  it("injects content.js when an existing tab has no receiving listener", async () => {
    const listeners = {};
    const sendMessage = vi
      .fn()
      .mockRejectedValueOnce(new Error("Could not establish connection. Receiving end does not exist."))
      .mockResolvedValue({ ok: true });
    const executeScript = vi.fn().mockResolvedValue([{ result: null }]);

    globalThis.chrome = createChromeMock(listeners, sendMessage, executeScript);
    await import("../public/background.js");

    const response = await new Promise((resolve) => {
      const keepChannelOpen = listeners.onMessage(
        { type: "SET_CONTROL_STATE", active: true, tabId: 17 },
        {},
        resolve,
      );
      expect(keepChannelOpen).toBe(true);
    });

    expect(response).toMatchObject({ ok: true, active: true, targetTabId: 17 });
    expect(executeScript).toHaveBeenCalledWith({
      target: { tabId: 17 },
      files: ["content.js"],
    });
    expect(sendMessage).toHaveBeenCalledWith(17, { type: "COMFORT_PING" });
    expect(sendMessage).toHaveBeenCalledWith(17, { type: "CONTROL_STATE", active: true });
  });

  it("restores active control after the service worker restarts", async () => {
    const sessionState = {};
    const firstListeners = {};
    const firstSendMessage = vi.fn().mockResolvedValue({ ok: true });
    globalThis.chrome = createChromeMock(firstListeners, firstSendMessage, vi.fn(), sessionState);
    await import("../public/background.js");

    const activation = await dispatchMessage(firstListeners, {
      type: "SET_CONTROL_STATE",
      active: true,
      tabId: 17,
    });
    expect(activation).toMatchObject({ ok: true, active: true, targetTabId: 17 });
    expect(sessionState.comfortControlState).toEqual({ active: true, targetTabId: 17 });

    vi.resetModules();
    const restartedListeners = {};
    const restartedSendMessage = vi.fn().mockResolvedValue({ ok: true });
    globalThis.chrome = createChromeMock(
      restartedListeners,
      restartedSendMessage,
      vi.fn(),
      sessionState,
    );
    await import("../public/background.js");

    const response = await dispatchMessage(restartedListeners, {
      type: "GESTURE_ACTION",
      action: { type: "SINGLE_CLICK" },
    });

    expect(response).toEqual({ ok: true });
    expect(restartedSendMessage).toHaveBeenCalledWith(17, {
      type: "GESTURE_ACTION",
      action: { type: "SINGLE_CLICK" },
    });
  });
});

function dispatchMessage(listeners, message) {
  return new Promise((resolve) => {
    const keepChannelOpen = listeners.onMessage(message, {}, resolve);
    expect(keepChannelOpen).toBe(true);
  });
}

function createChromeMock(listeners, sendMessage, executeScript, sessionState = {}) {
  return {
    action: {
      setBadgeBackgroundColor: vi.fn().mockResolvedValue(),
      setBadgeText: vi.fn().mockResolvedValue(),
    },
    runtime: {
      onConnect: { addListener: vi.fn((listener) => { listeners.onConnect = listener; }) },
      onInstalled: { addListener: vi.fn((listener) => { listeners.onInstalled = listener; }) },
      onMessage: { addListener: vi.fn((listener) => { listeners.onMessage = listener; }) },
      onStartup: { addListener: vi.fn((listener) => { listeners.onStartup = listener; }) },
      sendMessage: vi.fn().mockResolvedValue(),
    },
    scripting: { executeScript },
    sidePanel: { setPanelBehavior: vi.fn().mockResolvedValue() },
    storage: {
      session: {
        get: vi.fn(async (key) => ({ [key]: sessionState[key] })),
        remove: vi.fn(async (key) => { delete sessionState[key]; }),
        set: vi.fn(async (values) => { Object.assign(sessionState, values); }),
      },
    },
    tabs: {
      get: vi.fn().mockResolvedValue({ id: 17, url: "https://example.com/" }),
      onActivated: { addListener: vi.fn((listener) => { listeners.onActivated = listener; }) },
      onRemoved: { addListener: vi.fn((listener) => { listeners.onRemoved = listener; }) },
      onUpdated: { addListener: vi.fn((listener) => { listeners.onUpdated = listener; }) },
      sendMessage,
    },
  };
}
