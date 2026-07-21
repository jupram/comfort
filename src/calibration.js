import { DEFAULT_PROFILE } from "./gesture-engine.js";

export function deriveCalibration(openSamples, pinchSamples, currentProfile = {}) {
  const open = cleanSamples(openSamples);
  const pinched = cleanSamples(pinchSamples);

  if (open.length < 12 || pinched.length < 12) {
    throw new Error("Not enough hand samples were captured. Keep your hand visible and try again.");
  }

  const openGap = percentile(open, 0.25);
  const pinchGap = percentile(pinched, 0.75);
  const separation = openGap - pinchGap;

  if (separation < 0.08) {
    throw new Error("The two calibration poses were too similar. Spread, then fully pinch, your fingertips.");
  }

  const pinchEnter = clamp(pinchGap + separation * 0.28, 0.08, 0.6);
  const pinchExit = clamp(
    pinchEnter + Math.max(0.055, separation * 0.18),
    pinchEnter + 0.04,
    0.78,
  );

  return {
    ...DEFAULT_PROFILE,
    ...currentProfile,
    pinchEnter,
    pinchExit,
  };
}

function cleanSamples(samples) {
  return samples
    .filter((sample) => Number.isFinite(sample) && sample > 0 && sample < 3)
    .sort((a, b) => a - b);
}

function percentile(sorted, position) {
  const index = (sorted.length - 1) * position;
  const lower = Math.floor(index);
  const upper = Math.ceil(index);
  if (lower === upper) return sorted[lower];
  return sorted[lower] + (sorted[upper] - sorted[lower]) * (index - lower);
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}
