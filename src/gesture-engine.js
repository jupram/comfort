export const DEFAULT_PROFILE = Object.freeze({
  pinchEnter: 0.28,
  pinchExit: 0.4,
  scrollGain: 2200,
});

const MCP = [5, 9, 13, 17];
const PIP = [6, 10, 14, 18];
const DIP = [7, 11, 15, 19];
const TIP = [8, 12, 16, 20];

export class GestureEngine {
  constructor(profile = {}, options = {}) {
    this.profile = { ...DEFAULT_PROFILE, ...profile };
    this.options = {
      startHoldMs: 500,
      stopHoldMs: 500,
      doubleClickMs: 380,
      pinchHoldMs: 70,
      pinchReleaseMs: 55,
      movementDeadZone: 0.0025,
      maxScrollStep: 72,
      ...options,
    };
    this.active = false;
    this.poseCandidate = null;
    this.poseSince = 0;
    this.poseLatched = null;
    this.previousFingerY = null;
    this.pinchDown = false;
    this.pinchCandidateSince = null;
    this.pinchReleaseSince = null;
    this.pendingPinchAt = null;
  }

  setProfile(profile) {
    this.profile = { ...this.profile, ...profile };
  }

  forceStop() {
    const wasActive = this.active;
    this.active = false;
    this.resetInteractionState();
    this.resetPoseState();
    return wasActive;
  }

  update(landmarks, timestampMs) {
    const events = [];
    const now = Number(timestampMs) || 0;

    if (!Array.isArray(landmarks) || landmarks.length < 21) {
      this.previousFingerY = null;
      this.pinchDown = false;
      this.pinchCandidateSince = null;
      this.pinchReleaseSince = null;
      this.resetPoseState();
      this.flushPendingSingleClick(now, events);
      return this.result("No hand", null, events);
    }

    const metrics = getFingerMetrics(landmarks);
    const fingers = metrics.map(isExtended);
    const openHand = fingers.every(Boolean);
    const indexAndMiddleDeployed = isPinchFingerDeployed(metrics[0]) && isPinchFingerDeployed(metrics[1]);
    const ringAndPinkyFolded = isFolded(metrics[2]) && isFolded(metrics[3]);
    const pinchPose = indexAndMiddleDeployed && ringAndPinkyFolded;
    const closedFist = metrics.every(isStronglyCurled) && !pinchPose;
    const twoFingerPose = fingers[0] && fingers[1] && !fingers[2] && !fingers[3];
    const pinchRatio = getPinchRatio(landmarks);

    if (!this.active && openHand) {
      if (this.holdPose("open", now, this.options.startHoldMs)) {
        this.active = true;
        this.resetInteractionState();
        events.push({ type: "CONTROL_STARTED" });
      }
    } else if (this.active && closedFist) {
      if (this.holdPose("fist", now, this.options.stopHoldMs)) {
        this.active = false;
        this.resetInteractionState();
        events.push({ type: "CONTROL_STOPPED" });
      }
    } else {
      this.resetPoseState();
    }

    if (!this.active) {
      return this.result(openHand ? "Hold open hand to start" : "Ready", pinchRatio, events);
    }

    this.flushPendingSingleClick(now, events);

    if (pinchPose) {
      this.updatePinchState(pinchRatio, now, events);
    } else {
      this.pinchCandidateSince = null;
      this.updatePinchRelease(now);
    }

    if (twoFingerPose) {
      if (!this.pinchDown && this.pinchCandidateSince === null) {
        const meanY = (landmarks[8].y + landmarks[12].y) / 2;
        if (this.previousFingerY !== null) {
          const movement = meanY - this.previousFingerY;
          if (Math.abs(movement) >= this.options.movementDeadZone) {
            const delta = clamp(
              movement * this.profile.scrollGain,
              -this.options.maxScrollStep,
              this.options.maxScrollStep,
            );
            events.push({ type: "SCROLL", delta });
          }
        }
        this.previousFingerY = meanY;
      }
    } else {
      this.previousFingerY = null;
    }

    const pose = closedFist
      ? "Hold fist to stop"
      : this.pinchDown || (pinchPose && pinchRatio <= this.profile.pinchEnter)
        ? "Pinch"
        : twoFingerPose
          ? "Two-finger scroll"
          : "Control active";

    return this.result(pose, pinchRatio, events);
  }

  holdPose(name, now, durationMs) {
    if (this.poseCandidate !== name) {
      this.poseCandidate = name;
      this.poseSince = now;
      this.poseLatched = null;
      return false;
    }

    if (this.poseLatched === name || now - this.poseSince < durationMs) return false;
    this.poseLatched = name;
    return true;
  }

  updatePinchState(pinchRatio, now, events) {
    if (!this.pinchDown) {
      if (pinchRatio > this.profile.pinchEnter) {
        this.pinchCandidateSince = null;
        return;
      }

      if (this.pinchCandidateSince === null) {
        this.pinchCandidateSince = now;
        return;
      }

      if (now - this.pinchCandidateSince < this.options.pinchHoldMs) return;

      this.pinchDown = true;
      this.pinchCandidateSince = null;
      this.pinchReleaseSince = null;
      this.previousFingerY = null;

      if (
        this.pendingPinchAt !== null &&
        now - this.pendingPinchAt <= this.options.doubleClickMs
      ) {
        this.pendingPinchAt = null;
        events.push({ type: "DOUBLE_CLICK" });
      } else {
        this.pendingPinchAt = now;
      }
      return;
    }

    if (pinchRatio >= this.profile.pinchExit) this.updatePinchRelease(now);
    else this.pinchReleaseSince = null;
  }

  updatePinchRelease(now) {
    if (!this.pinchDown) {
      this.pinchReleaseSince = null;
      return;
    }

    if (this.pinchReleaseSince === null) {
      this.pinchReleaseSince = now;
      return;
    }

    if (now - this.pinchReleaseSince >= this.options.pinchReleaseMs) {
      this.pinchDown = false;
      this.pinchReleaseSince = null;
    }
  }

  flushPendingSingleClick(now, events) {
    if (
      this.pendingPinchAt !== null &&
      !this.pinchDown &&
      now - this.pendingPinchAt > this.options.doubleClickMs
    ) {
      this.pendingPinchAt = null;
      events.push({ type: "SINGLE_CLICK" });
    }
  }

  resetInteractionState() {
    this.previousFingerY = null;
    this.pinchDown = false;
    this.pinchCandidateSince = null;
    this.pinchReleaseSince = null;
    this.pendingPinchAt = null;
  }

  resetPoseState() {
    this.poseCandidate = null;
    this.poseSince = 0;
    this.poseLatched = null;
  }

  result(pose, pinchRatio, events) {
    return {
      active: this.active,
      pose,
      pinchRatio,
      events,
    };
  }
}

export function getExtendedFingers(landmarks) {
  return getFingerMetrics(landmarks).map(isExtended);
}

export function getFingerMetrics(landmarks) {
  const wrist = landmarks[0];
  return TIP.map((tipIndex, index) => {
    const mcp = landmarks[MCP[index]];
    const pip = landmarks[PIP[index]];
    const dip = landmarks[DIP[index]];
    const tip = landmarks[tipIndex];
    const pathLength = distance(mcp, pip) + distance(pip, dip) + distance(dip, tip);

    return {
      pipAngle: jointAngle(mcp, pip, dip),
      straightness: pathLength > 0.0001 ? distance(mcp, tip) / pathLength : 0,
      wristReach: distance(pip, wrist) > 0.0001
        ? distance(tip, wrist) / distance(pip, wrist)
        : 0,
    };
  });
}

export function getPinchRatio(landmarks) {
  if (!Array.isArray(landmarks) || landmarks.length < 21) return Number.NaN;
  const palmWidth = distance(landmarks[5], landmarks[17]);
  if (palmWidth < 0.0001) return Number.POSITIVE_INFINITY;
  return distance(landmarks[8], landmarks[12]) / palmWidth;
}

function distance(a, b) {
  return Math.hypot(a.x - b.x, a.y - b.y, (a.z ?? 0) - (b.z ?? 0));
}

function jointAngle(a, vertex, c) {
  const first = { x: a.x - vertex.x, y: a.y - vertex.y, z: (a.z ?? 0) - (vertex.z ?? 0) };
  const second = { x: c.x - vertex.x, y: c.y - vertex.y, z: (c.z ?? 0) - (vertex.z ?? 0) };
  const firstLength = Math.hypot(first.x, first.y, first.z);
  const secondLength = Math.hypot(second.x, second.y, second.z);
  if (firstLength < 0.0001 || secondLength < 0.0001) return 0;
  const cosine = clamp(
    (first.x * second.x + first.y * second.y + first.z * second.z) / (firstLength * secondLength),
    -1,
    1,
  );
  return Math.acos(cosine) * (180 / Math.PI);
}

function isExtended(metric) {
  return metric.pipAngle >= 145 && metric.straightness >= 0.72 && metric.wristReach >= 1.05;
}

function isPinchFingerDeployed(metric) {
  return metric.pipAngle >= 128 && metric.straightness >= 0.56 && metric.wristReach >= 0.95;
}

function isFolded(metric) {
  return metric.pipAngle <= 138 || metric.wristReach < 1.02;
}

function isStronglyCurled(metric) {
  return metric.pipAngle <= 122 && metric.straightness <= 0.76 && metric.wristReach <= 1.06;
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}
