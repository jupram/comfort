import { describe, expect, it } from "vitest";
import { deriveCalibration } from "../src/calibration.js";
import { GestureEngine } from "../src/gesture-engine.js";

describe("GestureEngine", () => {
  it("starts only after a thumbs-up is held", () => {
    const engine = new GestureEngine({}, { startHoldMs: 500 });
    expect(engine.update(thumbsUp(), 0).active).toBe(false);
    const result = engine.update(thumbsUp(), 501);
    expect(result.active).toBe(true);
    expect(result.events).toContainEqual({ type: "CONTROL_STARTED" });
  });

  it("does not start for an open hand", () => {
    const engine = new GestureEngine({}, { startHoldMs: 500 });
    engine.update(openHand(), 0);
    const result = engine.update(openHand(), 700);

    expect(result.active).toBe(false);
    expect(result.events).not.toContainEqual({ type: "CONTROL_STARTED" });
  });

  it("does not start for a sideways thumb", () => {
    const engine = new GestureEngine({}, { startHoldMs: 500 });
    engine.update(sidewaysThumb(), 0);
    const result = engine.update(sidewaysThumb(), 700);

    expect(result.active).toBe(false);
    expect(result.events).not.toContainEqual({ type: "CONTROL_STARTED" });
  });

  it("does not interpret a held thumbs-up as a stop gesture", () => {
    const engine = activeEngine();
    engine.update(thumbsUp(), 600);
    const result = engine.update(thumbsUp(), 1200);

    expect(result.active).toBe(true);
    expect(result.events).not.toContainEqual({ type: "CONTROL_STOPPED" });
  });

  it("blocks scrolling until control has started", () => {
    const engine = new GestureEngine();
    engine.update(twoFingers(0), 0);
    const result = engine.update(twoFingers(0.03), 16);
    expect(result.events).not.toContainEqual(expect.objectContaining({ type: "SCROLL" }));
  });

  it("scrolls in the physical two-finger movement direction", () => {
    const engine = activeEngine();
    engine.update(twoFingers(0), 600);

    const down = engine.update(twoFingers(0.03), 616);
    expect(down.events.find((event) => event.type === "SCROLL").delta).toBeGreaterThan(0);

    const up = engine.update(twoFingers(-0.01), 1216);
    expect(up.events.find((event) => event.type === "SCROLL").delta).toBeLessThan(0);
  });

  it("waits 600 ms before changing from scrolling down to up", () => {
    const engine = activeEngine();
    engine.update(twoFingers(0), 600);

    expect(scrollDelta(engine.update(twoFingers(0.03), 616))).toBeGreaterThan(0);
    expect(scrollDelta(engine.update(twoFingers(0), 800))).toBeUndefined();
    expect(scrollDelta(engine.update(twoFingers(-0.03), 1215))).toBeUndefined();
    expect(scrollDelta(engine.update(twoFingers(-0.06), 1216))).toBeLessThan(0);
  });

  it("waits 600 ms before changing from scrolling up to down", () => {
    const engine = activeEngine();
    engine.update(twoFingers(0), 600);

    expect(scrollDelta(engine.update(twoFingers(-0.03), 616))).toBeLessThan(0);
    expect(scrollDelta(engine.update(twoFingers(0), 800))).toBeUndefined();
    expect(scrollDelta(engine.update(twoFingers(0.03), 1215))).toBeUndefined();
    expect(scrollDelta(engine.update(twoFingers(0.06), 1216))).toBeGreaterThan(0);
  });

  it("continues scrolling in the same direction without the direction-change delay", () => {
    const engine = activeEngine();
    engine.update(twoFingers(0), 600);

    expect(scrollDelta(engine.update(twoFingers(0.03), 616))).toBeGreaterThan(0);
    expect(scrollDelta(engine.update(twoFingers(0.06), 632))).toBeGreaterThan(0);
  });

  it("turns a held pinch into a single click", () => {
    const engine = activeEngine();
    engine.update(pinchedFingers(), 600);
    const result = engine.update(pinchedFingers(), 675);
    expect(result.events).toContainEqual({ type: "SINGLE_CLICK" });
  });

  it("treats two pinches as two single clicks", () => {
    const engine = activeEngine();
    engine.update(pinchedFingers(), 600);
    const first = engine.update(pinchedFingers(), 675);
    engine.update(twoFingers(), 700);
    engine.update(twoFingers(), 760);
    engine.update(pinchedFingers(), 800);
    const second = engine.update(pinchedFingers(), 875);

    expect(first.events).toContainEqual({ type: "SINGLE_CLICK" });
    expect(second.events).toContainEqual({ type: "SINGLE_CLICK" });
  });

  it("goes back after the left-pointing gesture is held", () => {
    const engine = activeEngine();

    expect(engine.update(pointingLeft(), 600).events).not.toContainEqual({ type: "BROWSER_BACK" });
    expect(engine.update(pointingLeft(), 899).events).not.toContainEqual({ type: "BROWSER_BACK" });
    const result = engine.update(pointingLeft(), 900);

    expect(result.pose).toBe("Hold pointing left for browser back");
    expect(result.events).toContainEqual({ type: "BROWSER_BACK" });
    expect(engine.update(pointingLeft(), 1200).events).not.toContainEqual({ type: "BROWSER_BACK" });
  });

  it("does not navigate back before control starts or when pointing right", () => {
    const inactiveEngine = new GestureEngine();
    inactiveEngine.update(pointingLeft(), 0);
    expect(inactiveEngine.update(pointingLeft(), 500).events).not.toContainEqual({
      type: "BROWSER_BACK",
    });

    const engine = activeEngine();
    engine.update(pointingRight(), 600);
    expect(engine.update(pointingRight(), 1000).events).not.toContainEqual({ type: "BROWSER_BACK" });
  });

  it("does not stop control while a pinch is held", () => {
    const engine = activeEngine();
    engine.update(pinchedFingers(), 600);
    const result = engine.update(pinchedFingers(), 1100);

    expect(result.active).toBe(true);
    expect(result.pose).toBe("Pinch");
    expect(result.events).not.toContainEqual({ type: "CONTROL_STOPPED" });
  });

  it("rejects a one-frame pinch spike", () => {
    const engine = activeEngine();
    engine.update(pinchedFingers(), 600);
    engine.update(twoFingers(), 630);
    const result = engine.update(twoFingers(), 1100);

    expect(result.events).not.toContainEqual({ type: "SINGLE_CLICK" });
  });

  it("stops after a closed fist is held", () => {
    const engine = activeEngine();
    engine.update(closedFist(), 600);
    const result = engine.update(closedFist(), 1101);
    expect(result.active).toBe(false);
    expect(result.events).toContainEqual({ type: "CONTROL_STOPPED" });
  });
});

describe("deriveCalibration", () => {
  it("creates separated pinch enter and exit thresholds", () => {
    const profile = deriveCalibration(
      Array.from({ length: 30 }, (_, index) => 0.72 + index * 0.001),
      Array.from({ length: 30 }, (_, index) => 0.13 + index * 0.001),
    );
    expect(profile.pinchEnter).toBeGreaterThan(0.13);
    expect(profile.pinchEnter).toBeLessThan(0.72);
    expect(profile.pinchExit).toBeGreaterThan(profile.pinchEnter);
  });

  it("rejects calibration poses that are too similar", () => {
    expect(() => deriveCalibration(Array(20).fill(0.4), Array(20).fill(0.36))).toThrow(
      /too similar/i,
    );
  });
});

function activeEngine() {
  const engine = new GestureEngine();
  engine.update(thumbsUp(), 0);
  engine.update(thumbsUp(), 501);
  return engine;
}

function scrollDelta(result) {
  return result.events.find((event) => event.type === "SCROLL")?.delta;
}

function openHand() {
  return makeHand([true, true, true, true]);
}

function closedFist() {
  return makeHand([false, false, false, false]);
}

function thumbsUp() {
  const points = makeHand([false, false, false, false]);
  points[1] = { x: 0.43, y: 0.78, z: 0 };
  points[2] = { x: 0.4, y: 0.67, z: 0 };
  points[3] = { x: 0.4, y: 0.48, z: 0 };
  points[4] = { x: 0.4, y: 0.27, z: 0 };
  return points;
}

function sidewaysThumb() {
  const points = makeHand([false, false, false, false]);
  points[1] = { x: 0.43, y: 0.78, z: 0 };
  points[2] = { x: 0.4, y: 0.67, z: 0 };
  points[3] = { x: 0.28, y: 0.66, z: 0 };
  points[4] = { x: 0.16, y: 0.65, z: 0 };
  return points;
}

function pointingLeft() {
  const points = thumbsUp();
  points[6] = { x: 0.55, y: 0.65, z: 0 };
  points[7] = { x: 0.68, y: 0.62, z: 0 };
  points[8] = { x: 0.82, y: 0.6, z: 0 };
  return points;
}

function pointingRight() {
  const points = thumbsUp();
  points[6] = { x: 0.3, y: 0.65, z: 0 };
  points[7] = { x: 0.18, y: 0.62, z: 0 };
  points[8] = { x: 0.06, y: 0.6, z: 0 };
  return points;
}

function twoFingers(yOffset = 0) {
  return makeHand([true, true, false, false], yOffset, false);
}

function pinchedFingers() {
  return makeHand([true, true, false, false], 0, true);
}

function makeHand(extended, yOffset = 0, pinched = false) {
  const points = Array.from({ length: 21 }, () => ({ x: 0.5, y: 0.7, z: 0 }));
  points[0] = { x: 0.5, y: 0.9, z: 0 };
  points[5] = { x: 0.42, y: 0.68, z: 0 };
  points[9] = { x: 0.48, y: 0.66, z: 0 };
  points[13] = { x: 0.54, y: 0.68, z: 0 };
  points[17] = { x: 0.58, y: 0.7, z: 0 };

  const pips = [6, 10, 14, 18];
  const dips = [7, 11, 15, 19];
  const tips = [8, 12, 16, 20];
  const xs = [0.42, 0.48, 0.54, 0.6];
  const pipYs = [0.5, 0.48, 0.52, 0.56];
  const tipYs = [0.25, 0.2, 0.28, 0.34];

  for (let index = 0; index < 4; index += 1) {
    if (extended[index]) {
      points[pips[index]] = { x: xs[index], y: pipYs[index] + yOffset, z: 0 };
      points[dips[index]] = {
        x: xs[index],
        y: (pipYs[index] + tipYs[index]) / 2 + yOffset,
        z: 0,
      };
      points[tips[index]] = { x: xs[index], y: tipYs[index] + yOffset, z: 0 };
    } else {
      points[pips[index]] = { x: xs[index], y: 0.54 + yOffset, z: 0 };
      points[dips[index]] = { x: xs[index] + 0.035, y: 0.62 + yOffset, z: 0 };
      points[tips[index]] = { x: xs[index] + 0.02, y: 0.7 + yOffset, z: 0 };
    }
  }

  if (pinched) {
    points[7] = { x: 0.44, y: 0.32 + yOffset, z: 0 };
    points[11] = { x: 0.475, y: 0.3 + yOffset, z: 0 };
    points[8] = { x: 0.47, y: 0.25 + yOffset, z: 0 };
    points[12] = { x: 0.485, y: 0.25 + yOffset, z: 0 };
  }

  return points;
}
