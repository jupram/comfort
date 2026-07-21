(() => {
  if (globalThis.__comfortGestureContentLoaded) return;
  globalThis.__comfortGestureContentLoaded = true;

  const extensionApi = globalThis.__comfortChromeMock ?? chrome;
  const HOST_ID = "comfort-gesture-control-root";
  let host = null;
  let reticle = null;
  let pendingScrollDelta = 0;
  let scrollFrameId = null;
  let cachedScrollTarget = null;
  let scrollTargetExpiresAt = 0;

  extensionApi.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    if (message?.type === "COMFORT_PING") {
      sendResponse({ ok: true });
      return;
    }

    if (message?.type === "CONTROL_STATE") {
      setControlActive(Boolean(message.active));
      sendResponse({ ok: true });
      return;
    }

    if (message?.type === "GESTURE_ACTION") {
      performAction(message.action);
      sendResponse({ ok: true });
    }
  });

  function ensureOverlay() {
    host = document.getElementById(HOST_ID);
    if (host) return;

    host = document.createElement("div");
    host.id = HOST_ID;
    host.style.cssText = [
      "all:initial",
      "position:fixed",
      "inset:0",
      "z-index:2147483647",
      "pointer-events:none",
      "display:none",
    ].join(";");

    const shadow = host.attachShadow({ mode: "closed" });
    const style = document.createElement("style");
    style.textContent = `
      :host { all: initial; }
      .halo {
        position: fixed;
        inset: 4px;
        border: 3px solid rgba(38, 208, 139, .92);
        border-radius: 14px;
        box-shadow: inset 0 0 24px rgba(38, 208, 139, .26), 0 0 18px rgba(38, 208, 139, .45);
        animation: comfort-breathe 2.2s ease-in-out infinite;
      }
      .label {
        position: fixed;
        top: 14px;
        left: 50%;
        transform: translateX(-50%);
        padding: 7px 12px;
        border: 1px solid rgba(255,255,255,.22);
        border-radius: 999px;
        background: rgba(7, 24, 20, .86);
        color: #baf7dd;
        font: 600 12px/1.2 system-ui, sans-serif;
        letter-spacing: .04em;
        box-shadow: 0 8px 26px rgba(0,0,0,.28);
      }
      .reticle {
        position: fixed;
        left: 50%;
        top: 50%;
        width: 30px;
        height: 30px;
        transform: translate(-50%, -50%);
        border: 2px solid #f4fff9;
        border-radius: 50%;
        box-shadow: 0 0 0 4px rgba(18, 167, 105, .45), 0 0 18px rgba(18, 167, 105, .65);
        transition: transform 100ms ease, background 100ms ease;
      }
      .reticle::before, .reticle::after {
        content: "";
        position: absolute;
        background: #f4fff9;
      }
      .reticle::before { width: 10px; height: 2px; left: 8px; top: 12px; }
      .reticle::after { width: 2px; height: 10px; left: 12px; top: 8px; }
      .reticle.flash {
        transform: translate(-50%, -50%) scale(.72);
        background: rgba(255,255,255,.78);
      }
      @keyframes comfort-breathe {
        0%, 100% { opacity: .7; }
        50% { opacity: 1; }
      }
      @media (prefers-reduced-motion: reduce) {
        .halo { animation: none; }
        .reticle { transition: none; }
      }
    `;

    const halo = document.createElement("div");
    halo.className = "halo";
    halo.setAttribute("aria-hidden", "true");

    const label = document.createElement("div");
    label.className = "label";
    label.textContent = "GESTURE CONTROL ON";

    reticle = document.createElement("div");
    reticle.className = "reticle";
    reticle.setAttribute("aria-hidden", "true");

    shadow.append(style, halo, label, reticle);
    (document.documentElement || document.body).appendChild(host);
  }

  function setControlActive(active) {
    ensureOverlay();
    host.style.display = active ? "block" : "none";
    if (!active) resetPendingScroll();
  }

  function performAction(action) {
    if (!host || host.style.display === "none" || !action) return;

    if (action.type === "SCROLL") {
      scrollAtReticle(Number(action.delta) || 0);
      return;
    }

    if (action.type === "SINGLE_CLICK") clickAtReticle(false);
    if (action.type === "DOUBLE_CLICK") clickAtReticle(true);
  }

  function scrollAtReticle(delta) {
    if (Math.abs(delta) < 1) return;
    pendingScrollDelta += delta;
    if (scrollFrameId === null) scrollFrameId = requestAnimationFrame(flushScroll);
  }

  function flushScroll() {
    scrollFrameId = null;
    const delta = pendingScrollDelta;
    pendingScrollDelta = 0;
    if (!host || host.style.display === "none" || Math.abs(delta) < 1) return;

    const target = getScrollTarget(performance.now());
    if (target === document.scrollingElement) {
      window.scrollBy({ top: delta, left: 0, behavior: "auto" });
    } else {
      target.scrollBy({ top: delta, left: 0, behavior: "auto" });
    }
  }

  function resetPendingScroll() {
    if (scrollFrameId !== null) cancelAnimationFrame(scrollFrameId);
    scrollFrameId = null;
    pendingScrollDelta = 0;
    cachedScrollTarget = null;
    scrollTargetExpiresAt = 0;
  }

  function getScrollTarget(now) {
    if (
      cachedScrollTarget?.isConnected &&
      now < scrollTargetExpiresAt &&
      cachedScrollTarget.scrollHeight > cachedScrollTarget.clientHeight + 2
    ) {
      return cachedScrollTarget;
    }

    let element = document.elementFromPoint(innerWidth / 2, innerHeight / 2);
    while (element && element !== document.documentElement) {
      const style = getComputedStyle(element);
      const canScroll = /(auto|scroll)/.test(style.overflowY) && element.scrollHeight > element.clientHeight + 2;
      if (canScroll) {
        cachedScrollTarget = element;
        scrollTargetExpiresAt = now + 250;
        return element;
      }
      element = element.parentElement;
    }
    cachedScrollTarget = document.scrollingElement || document.documentElement;
    scrollTargetExpiresAt = now + 250;
    return cachedScrollTarget;
  }

  function clickAtReticle(doubleClick) {
    const x = innerWidth / 2;
    const y = innerHeight / 2;
    const hit = document.elementFromPoint(x, y);
    if (!hit || hit === host) return;
    const target = hit.closest?.("a, button, input, select, textarea, summary, [role='button'], [role='link'], [tabindex]") || hit;

    if (typeof target.focus === "function") {
      target.focus({ preventScroll: true });
    }

    const clicks = doubleClick ? 2 : 1;
    for (let detail = 1; detail <= clicks; detail += 1) {
      dispatchPointerSequence(target, x, y, detail);
      if (typeof target.click === "function") {
        target.click();
      } else {
        target.dispatchEvent(new MouseEvent("click", mouseOptions(x, y, detail)));
      }
    }

    if (doubleClick) {
      target.dispatchEvent(new MouseEvent("dblclick", mouseOptions(x, y, 2)));
    }

    reticle?.classList.add("flash");
    setTimeout(() => reticle?.classList.remove("flash"), 130);
  }

  function dispatchPointerSequence(target, x, y, detail) {
    if (typeof PointerEvent === "function") {
      target.dispatchEvent(new PointerEvent("pointerdown", pointerOptions(x, y, detail)));
    }
    target.dispatchEvent(new MouseEvent("mousedown", mouseOptions(x, y, detail)));
    if (typeof PointerEvent === "function") {
      target.dispatchEvent(new PointerEvent("pointerup", pointerOptions(x, y, detail)));
    }
    target.dispatchEvent(new MouseEvent("mouseup", mouseOptions(x, y, detail)));
  }

  function mouseOptions(x, y, detail) {
    return {
      bubbles: true,
      cancelable: true,
      composed: true,
      view: window,
      button: 0,
      buttons: 0,
      clientX: x,
      clientY: y,
      detail,
    };
  }

  function pointerOptions(x, y, detail) {
    return {
      ...mouseOptions(x, y, detail),
      pointerId: 1,
      pointerType: "mouse",
      isPrimary: true,
    };
  }
})();
