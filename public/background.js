const control = {
  active: false,
  targetTabId: null,
};

let panelPort = null;

chrome.runtime.onInstalled.addListener(() => {
  chrome.sidePanel.setPanelBehavior({ openPanelOnActionClick: true }).catch(() => {});
  clearBadge();
});

chrome.runtime.onStartup.addListener(clearBadge);

chrome.runtime.onConnect.addListener((port) => {
  if (port.name !== "comfort-control-panel") return;

  panelPort = port;
  port.onDisconnect.addListener(() => {
    if (panelPort === port) panelPort = null;
    void deactivateControl("panel-closed");
  });
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type === "SET_CONTROL_STATE") {
    const task = message.active
      ? activateControl(message.tabId)
      : deactivateControl(message.reason ?? "gesture");

    task
      .then(() => sendResponse({ ok: true, ...control }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message?.type === "GESTURE_ACTION") {
    forwardGesture(message.action)
      .then(() => sendResponse({ ok: true }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message?.type === "GET_CONTROL_STATE") {
    sendResponse({ ...control });
  }

  return false;
});

chrome.tabs.onActivated.addListener(({ tabId }) => {
  if (control.active && tabId !== control.targetTabId) {
    void deactivateControl("tab-changed", true);
  }
});

chrome.tabs.onRemoved.addListener((tabId) => {
  if (control.active && tabId === control.targetTabId) {
    void deactivateControl("tab-closed", true);
  }
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (control.active && tabId === control.targetTabId && changeInfo.status === "complete") {
    void ensureContentScript(tabId)
      .then(() => sendToTab(tabId, { type: "CONTROL_STATE", active: true }))
      .catch(() => deactivateControl("target-unavailable", true));
  }
});

async function activateControl(tabId) {
  if (!Number.isInteger(tabId)) throw new Error("No controllable tab is active.");

  const tab = await chrome.tabs.get(tabId);
  if (!tab.url || !/^(https?|file):/.test(tab.url)) {
    throw new Error("This browser page does not allow gesture control.");
  }

  await ensureContentScript(tabId);

  if (control.active && control.targetTabId !== tabId) {
    await sendToTab(control.targetTabId, { type: "CONTROL_STATE", active: false }).catch(() => {});
  }

  control.active = true;
  control.targetTabId = tabId;
  await sendToTab(tabId, { type: "CONTROL_STATE", active: true });
  await setBadge(tabId, true);
}

async function deactivateControl(reason, notifyPanel = false) {
  const previousTabId = control.targetTabId;
  control.active = false;
  control.targetTabId = null;

  if (Number.isInteger(previousTabId)) {
    await sendToTab(previousTabId, { type: "CONTROL_STATE", active: false }).catch(() => {});
    await setBadge(previousTabId, false).catch(() => {});
  }

  if (notifyPanel) {
    chrome.runtime.sendMessage({ type: "CONTROL_FORCED_STOP", reason }).catch(() => {});
  }
}

async function forwardGesture(action) {
  if (!control.active || !Number.isInteger(control.targetTabId)) {
    throw new Error("Control is not active.");
  }

  const message = { type: "GESTURE_ACTION", action };

  try {
    await sendToTab(control.targetTabId, message);
  } catch {
    await ensureContentScript(control.targetTabId);
    await sendToTab(control.targetTabId, message);
  }
}

async function ensureContentScript(tabId) {
  try {
    const response = await sendToTab(tabId, { type: "COMFORT_PING" });
    if (response?.ok) return;
  } catch {
    // Tabs that were open before an extension reload need a one-time injection.
  }

  try {
    await chrome.scripting.executeScript({
      target: { tabId },
      files: ["content.js"],
    });
    const response = await sendToTab(tabId, { type: "COMFORT_PING" });
    if (!response?.ok) throw new Error("The page did not initialize gesture control.");
  } catch (error) {
    const detail = error?.message || "Unknown browser error";
    if (/cannot access|missing host permission|not allowed|chrome:\/\/|edge:\/\//i.test(detail)) {
      throw new Error(
        "This browser page is protected and does not allow gesture control. Open a regular website and try again.",
        { cause: error },
      );
    }
    throw new Error("Comfort could not connect to this page. Reload the tab and try again.", { cause: error });
  }
}

async function sendToTab(tabId, message) {
  if (!Number.isInteger(tabId)) throw new Error("The controlled tab is unavailable.");
  return chrome.tabs.sendMessage(tabId, message);
}

async function setBadge(tabId, active) {
  await chrome.action.setBadgeBackgroundColor({
    tabId,
    color: active ? "#16a36a" : "#667085",
  });
  await chrome.action.setBadgeText({ tabId, text: active ? "ON" : "" });
}

function clearBadge() {
  chrome.action.setBadgeText({ text: "" }).catch(() => {});
}
