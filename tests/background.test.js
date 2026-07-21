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
});

function createChromeMock(listeners, sendMessage, executeScript) {
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
    tabs: {
      get: vi.fn().mockResolvedValue({ id: 17, url: "https://example.com/" }),
      onActivated: { addListener: vi.fn((listener) => { listeners.onActivated = listener; }) },
      onRemoved: { addListener: vi.fn((listener) => { listeners.onRemoved = listener; }) },
      onUpdated: { addListener: vi.fn((listener) => { listeners.onUpdated = listener; }) },
      sendMessage,
    },
  };
}
