import { FilesetResolver, HandLandmarker } from "@mediapipe/tasks-vision";
import "./styles.css";
import { deriveCalibration } from "./calibration.js";
import { GestureActionQueue } from "./gesture-action-queue.js";
import { DEFAULT_PROFILE, GestureEngine, getPinchRatio } from "./gesture-engine.js";

const extensionApi = globalThis.__comfortChromeMock ?? chrome;

const PROFILE_KEY = "comfortCalibrationProfile";
const BASE_SCROLL_GAIN = DEFAULT_PROFILE.scrollGain;
const CAMERA_FRAME_RATE = 24;
const HIGHLIGHTED_TIPS = new Set([4, 8, 12]);
const CONNECTIONS = [
  [0, 1], [1, 2], [2, 3], [3, 4],
  [0, 5], [5, 6], [6, 7], [7, 8],
  [5, 9], [9, 10], [10, 11], [11, 12],
  [9, 13], [13, 14], [14, 15], [15, 16],
  [13, 17], [17, 18], [18, 19], [19, 20], [0, 17],
];

const elements = {
  video: document.querySelector("#camera"),
  canvas: document.querySelector("#landmark-canvas"),
  cameraCard: document.querySelector(".camera-card"),
  cameraButton: document.querySelector("#camera-button"),
  cameraPermissionButton: document.querySelector("#camera-permission-button"),
  calibrateButton: document.querySelector("#calibrate-button"),
  stopButton: document.querySelector("#stop-button"),
  status: document.querySelector("#control-status"),
  statusText: document.querySelector("#control-status-text"),
  gestureReadout: document.querySelector("#gesture-readout"),
  handIndicator: document.querySelector("#hand-indicator"),
  notice: document.querySelector("#notice"),
  calibrationState: document.querySelector("#calibration-state"),
  scrollSpeed: document.querySelector("#scroll-speed"),
  speedOutput: document.querySelector("#speed-output"),
};
const landmarkContext = elements.canvas.getContext("2d");

if (!landmarkContext) throw new Error("Canvas rendering is unavailable.");

let profile = { ...DEFAULT_PROFILE };
let engine = new GestureEngine(profile);
let handLandmarker = null;
let stream = null;
let animationFrameId = null;
let videoFrameCallbackId = null;
let lastVideoTime = -1;
let cameraReady = false;
let confirmedActive = false;
let calibrationSession = null;
let calibrationRunning = false;
let handlingControlChange = false;

const panelPort = extensionApi.runtime.connect({ name: "comfort-control-panel" });
const gestureActions = new GestureActionQueue(sendGestureAction, handleGestureDeliveryError);

void initialize().catch((error) => {
  setNotice(`Could not initialize Comfort: ${error?.message || "Unknown error"}`, "error");
});

elements.cameraButton.addEventListener("click", () => {
  if (cameraReady) void stopCamera();
  else void startCamera();
});

elements.cameraPermissionButton.addEventListener("click", () => {
  void openCameraPermissionPage();
});

elements.stopButton.addEventListener("click", () => {
  void stopControl("manual");
});

elements.calibrateButton.addEventListener("click", () => {
  void runCalibration();
});

elements.scrollSpeed.addEventListener("input", () => {
  const multiplier = Number(elements.scrollSpeed.value);
  elements.speedOutput.value = `${multiplier.toFixed(1)}×`;
  profile.scrollGain = BASE_SCROLL_GAIN * multiplier;
  engine.setProfile(profile);
});

elements.scrollSpeed.addEventListener("change", () => {
  void saveProfile();
});

extensionApi.runtime.onMessage.addListener((message) => {
  if (message?.type !== "CONTROL_FORCED_STOP") return;
  confirmedActive = false;
  engine.forceStop();
  setActiveUi(false);
  setNotice(forcedStopMessage(message.reason), "info");
});

window.addEventListener("pagehide", () => {
  if (confirmedActive) {
    void extensionApi.runtime.sendMessage({
      type: "SET_CONTROL_STATE",
      active: false,
      reason: "panel-closed",
    }).catch(() => {});
  }
  cancelFrameLoop();
  gestureActions.clear();
  stream?.getTracks().forEach((track) => track.stop());
  try {
    handLandmarker?.close();
  } catch {
    // The detector may already be disposed after a runtime failure.
  }
  handLandmarker = null;
  try {
    panelPort.disconnect();
  } catch {
    // The service worker may already have disconnected the port.
  }
});

async function initialize() {
  const stored = await extensionApi.storage.local.get(PROFILE_KEY);
  if (stored[PROFILE_KEY]) {
    profile = { ...DEFAULT_PROFILE, ...stored[PROFILE_KEY] };
    engine = new GestureEngine(profile);
    elements.calibrationState.textContent = "Calibrated";
  }

  const speed = clamp(profile.scrollGain / BASE_SCROLL_GAIN, 0.5, 2);
  elements.scrollSpeed.value = speed.toFixed(1);
  elements.speedOutput.value = `${speed.toFixed(1)}×`;

  const state = await extensionApi.runtime.sendMessage({ type: "GET_CONTROL_STATE" }).catch(() => null);
  if (state?.active) {
    await extensionApi.runtime.sendMessage({ type: "SET_CONTROL_STATE", active: false, reason: "panel-reopened" });
  }
}

async function startCamera() {
  elements.cameraButton.disabled = true;
  setNotice("Starting the local hand tracker…", "info");

  try {
    stream = await navigator.mediaDevices.getUserMedia({
      audio: false,
      video: {
        facingMode: "user",
        width: { ideal: 640 },
        height: { ideal: 480 },
        frameRate: { ideal: CAMERA_FRAME_RATE, max: CAMERA_FRAME_RATE },
      },
    });

    await ensureHandLandmarker();

    elements.video.srcObject = stream;
    await elements.video.play();
    resizeCanvas();

    cameraReady = true;
    elements.cameraPermissionButton.hidden = true;
    elements.cameraCard.dataset.ready = "true";
    elements.cameraButton.innerHTML = '<span class="button-camera-icon" aria-hidden="true"></span>Disable camera';
    elements.calibrateButton.disabled = false;
    elements.gestureReadout.textContent = "Show your hand";
    setNotice("Hold a thumbs-up to start control.", "success");
    lastVideoTime = -1;
    scheduleNextFrame();
  } catch (error) {
    stream?.getTracks().forEach((track) => track.stop());
    stream = null;
    elements.cameraPermissionButton.hidden = error?.name !== "NotAllowedError";
    setNotice(cameraErrorMessage(error), "error");
  } finally {
    elements.cameraButton.disabled = false;
  }
}

async function openCameraPermissionPage() {
  elements.cameraPermissionButton.disabled = true;
  try {
    await extensionApi.tabs.create({
      url: extensionApi.runtime.getURL("camera-permission.html"),
    });
    setNotice("Camera permission opened in a new tab. Allow access there, then return and enable the camera.", "info");
  } catch (error) {
    setNotice(`Could not open camera permission: ${error?.message || "Unknown error"}`, "error");
  } finally {
    elements.cameraPermissionButton.disabled = false;
  }
}

async function stopCamera() {
  if (calibrationRunning) return;
  elements.cameraButton.disabled = true;
  await stopControl("camera-off");

  cancelFrameLoop();
  stream?.getTracks().forEach((track) => track.stop());
  stream = null;
  elements.video.srcObject = null;
  clearCanvas();

  cameraReady = false;
  elements.cameraCard.dataset.ready = "false";
  elements.cameraButton.innerHTML = '<span class="button-camera-icon" aria-hidden="true"></span>Enable camera';
  elements.cameraButton.disabled = false;
  elements.calibrateButton.disabled = true;
  elements.handIndicator.dataset.visible = "false";
  elements.handIndicator.textContent = "No hand";
  elements.gestureReadout.textContent = "Enable the camera";
  setNotice("Camera stopped. Gesture control is off.", "info");
}

async function ensureHandLandmarker() {
  if (handLandmarker) return handLandmarker;

  const vision = await FilesetResolver.forVisionTasks(extensionApi.runtime.getURL("wasm"));
  const options = {
    baseOptions: {
      modelAssetPath: extensionApi.runtime.getURL("models/hand_landmarker.task"),
      delegate: "GPU",
    },
    runningMode: "VIDEO",
    numHands: 1,
    minHandDetectionConfidence: 0.6,
    minHandPresenceConfidence: 0.6,
    minTrackingConfidence: 0.55,
  };

  try {
    handLandmarker = await HandLandmarker.createFromOptions(vision, options);
  } catch {
    options.baseOptions.delegate = "CPU";
    handLandmarker = await HandLandmarker.createFromOptions(vision, options);
  }

  return handLandmarker;
}

function scheduleNextFrame() {
  if (!cameraReady) return;
  if (typeof elements.video.requestVideoFrameCallback === "function") {
    videoFrameCallbackId = elements.video.requestVideoFrameCallback(processFrame);
  } else {
    animationFrameId = requestAnimationFrame(processFrame);
  }
}

function cancelFrameLoop() {
  if (animationFrameId !== null) cancelAnimationFrame(animationFrameId);
  if (videoFrameCallbackId !== null && typeof elements.video.cancelVideoFrameCallback === "function") {
    elements.video.cancelVideoFrameCallback(videoFrameCallbackId);
  }
  animationFrameId = null;
  videoFrameCallbackId = null;
}

function processFrame(timestamp) {
  animationFrameId = null;
  videoFrameCallbackId = null;
  if (!cameraReady || !handLandmarker) return;

  try {
    if (elements.video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA && elements.video.currentTime !== lastVideoTime) {
      lastVideoTime = elements.video.currentTime;
      const result = handLandmarker.detectForVideo(elements.video, timestamp);
      const landmarks = result.landmarks?.[0] ?? null;
      drawLandmarks(landmarks);
      updateHandVisibility(Boolean(landmarks));

      if (calibrationSession) {
        if (calibrationSession.collecting && landmarks) {
          const ratio = getPinchRatio(landmarks);
          if (Number.isFinite(ratio)) calibrationSession.samples.push(ratio);
        }
      } else {
        const recognition = engine.update(landmarks, timestamp);
        setText(elements.gestureReadout, recognition.pose);
        for (const event of recognition.events) void handleGestureEvent(event);
      }
    }
  } catch (error) {
    void handleTrackingFailure(error);
    return;
  }

  scheduleNextFrame();
}

async function handleTrackingFailure(error) {
  cameraReady = false;
  cancelFrameLoop();
  gestureActions.clear();
  stream?.getTracks().forEach((track) => track.stop());
  stream = null;
  elements.video.srcObject = null;
  clearCanvas();
  try {
    handLandmarker?.close();
  } catch {
    // The detector may already have disposed itself when it failed.
  }
  handLandmarker = null;
  await stopControl("tracking-error");

  elements.cameraCard.dataset.ready = "false";
  elements.cameraButton.innerHTML = '<span class="button-camera-icon" aria-hidden="true"></span>Enable camera';
  elements.cameraButton.disabled = false;
  elements.calibrateButton.disabled = true;
  updateHandVisibility(false);
  setText(elements.gestureReadout, "Tracking stopped");
  setNotice(`Hand tracking stopped: ${error?.message || "Unknown error"}. Enable the camera to retry.`, "error");
}

async function handleGestureEvent(event) {
  if (event.type === "CONTROL_STARTED") {
    await startControl();
    return;
  }

  if (event.type === "CONTROL_STOPPED") {
    await stopControl("fist", false);
    return;
  }

  if (!confirmedActive) return;

  const action = event.type === "SCROLL"
    ? { type: "SCROLL", delta: event.delta }
    : { type: event.type };

  gestureActions.enqueue(action);
}

async function sendGestureAction(action) {
  const response = await extensionApi.runtime.sendMessage({ type: "GESTURE_ACTION", action });
  if (!response?.ok) throw new Error(response?.error || "The controlled page is unavailable.");
}

async function handleGestureDeliveryError(error) {
  await stopControl("target-unavailable");
  setNotice(error?.message || "The controlled page is unavailable.", "error");
}

async function startControl() {
  if (handlingControlChange || confirmedActive) return;
  handlingControlChange = true;
  gestureActions.clear();

  try {
    const [tab] = await extensionApi.tabs.query({ active: true, currentWindow: true });
    const response = await extensionApi.runtime.sendMessage({
      type: "SET_CONTROL_STATE",
      active: true,
      tabId: tab?.id,
    });

    if (!response?.ok) throw new Error(response?.error || "Unable to activate this tab.");
    confirmedActive = true;
    setActiveUi(true);
    setNotice("Control is active. Make a fist to stop.", "success");
  } catch (error) {
    confirmedActive = false;
    engine.forceStop();
    setActiveUi(false);
    setNotice(error.message, "error");
  } finally {
    handlingControlChange = false;
  }
}

async function stopControl(reason, resetEngine = true) {
  gestureActions.clear();
  if (resetEngine) engine.forceStop();
  confirmedActive = false;
  setActiveUi(false);

  await extensionApi.runtime.sendMessage({
    type: "SET_CONTROL_STATE",
    active: false,
    reason,
  }).catch(() => {});

  if (reason === "fist") setNotice("Control stopped. Hold a thumbs-up to start again.", "info");
}

async function runCalibration() {
  if (!cameraReady || calibrationRunning) return;
  calibrationRunning = true;
  elements.calibrateButton.disabled = true;
  elements.cameraButton.disabled = true;
  elements.scrollSpeed.disabled = true;
  await stopControl("calibration");

  try {
    const openSamples = await collectCalibrationPhase(
      "Spread your index and middle fingers comfortably.",
      "Spread fingers",
    );
    const pinchSamples = await collectCalibrationPhase(
      "Keep your index and middle fingers forward, then bring only their tips together.",
      "Pinch fingertips",
    );

    profile = deriveCalibration(openSamples, pinchSamples, profile);
    engine.setProfile(profile);
    await saveProfile();
    elements.calibrationState.textContent = "Calibrated";
    elements.gestureReadout.textContent = "Calibration complete";
    setNotice("Calibration saved on this device.", "success");
  } catch (error) {
    setNotice(error.message, "error");
  } finally {
    calibrationSession = null;
    calibrationRunning = false;
    elements.calibrateButton.disabled = !cameraReady;
    elements.cameraButton.disabled = false;
    elements.scrollSpeed.disabled = false;
  }
}

async function collectCalibrationPhase(instruction, readout) {
  elements.gestureReadout.textContent = readout;
  setNotice(`${instruction} Get ready…`, "info");
  calibrationSession = { collecting: false, samples: [] };
  await delay(900);
  assertCalibrationAvailable();
  setNotice(`${instruction} Hold still.`, "success");
  calibrationSession.collecting = true;
  await delay(1700);
  assertCalibrationAvailable();
  calibrationSession.collecting = false;
  return calibrationSession.samples;
}

function assertCalibrationAvailable() {
  if (!cameraReady || !calibrationSession) {
    throw new Error("Calibration stopped because hand tracking became unavailable.");
  }
}

async function saveProfile() {
  await extensionApi.storage.local.set({ [PROFILE_KEY]: profile });
}

function setActiveUi(active) {
  document.body.dataset.active = String(active);
  elements.status.dataset.kind = active ? "active" : "idle";
  elements.statusText.textContent = active ? "Active" : "Off";
  elements.stopButton.disabled = !active;
}

function setNotice(message, kind) {
  elements.notice.textContent = message;
  elements.notice.dataset.kind = kind;
}

function updateHandVisibility(visible) {
  const value = String(visible);
  if (elements.handIndicator.dataset.visible !== value) elements.handIndicator.dataset.visible = value;
  setText(elements.handIndicator, visible ? "Hand found" : "No hand");
}

function resizeCanvas() {
  elements.canvas.width = elements.video.videoWidth || 640;
  elements.canvas.height = elements.video.videoHeight || 480;
}

function clearCanvas() {
  landmarkContext.clearRect(0, 0, elements.canvas.width, elements.canvas.height);
}

function drawLandmarks(landmarks) {
  const canvas = elements.canvas;
  const context = landmarkContext;
  if (canvas.width !== elements.video.videoWidth && elements.video.videoWidth) resizeCanvas();
  context.clearRect(0, 0, canvas.width, canvas.height);
  if (!landmarks) return;

  context.save();
  context.translate(canvas.width, 0);
  context.scale(-1, 1);
  context.lineWidth = 2;
  context.strokeStyle = "rgba(75, 224, 156, .72)";

  context.beginPath();
  for (const [start, end] of CONNECTIONS) {
    context.moveTo(landmarks[start].x * canvas.width, landmarks[start].y * canvas.height);
    context.lineTo(landmarks[end].x * canvas.width, landmarks[end].y * canvas.height);
  }
  context.stroke();

  context.fillStyle = "#4fe09b";
  context.beginPath();
  for (const [index, point] of landmarks.entries()) {
    if (HIGHLIGHTED_TIPS.has(index)) continue;
    context.moveTo(point.x * canvas.width + 3, point.y * canvas.height);
    context.arc(point.x * canvas.width, point.y * canvas.height, 3, 0, Math.PI * 2);
  }
  context.fill();
  context.fillStyle = "#f1fff8";
  context.beginPath();
  for (const index of HIGHLIGHTED_TIPS) {
    const point = landmarks[index];
    context.moveTo(point.x * canvas.width + 5, point.y * canvas.height);
    context.arc(point.x * canvas.width, point.y * canvas.height, 5, 0, Math.PI * 2);
  }
  context.fill();
  context.restore();
}

function setText(element, value) {
  if (element.textContent !== value) element.textContent = value;
}

function cameraErrorMessage(error) {
  if (error?.name === "NotAllowedError") return "Camera is blocked. Open camera permission below, allow access in the new tab, then retry.";
  if (error?.name === "NotFoundError") return "No camera was found on this device.";
  if (error?.name === "NotReadableError") return "The camera is busy or unavailable. Close other apps using it, then retry.";
  return `Could not start hand tracking: ${error?.message || "Unknown error"}`;
}

function forcedStopMessage(reason) {
  if (reason === "tab-changed") return "Control stopped because you changed tabs.";
  if (reason === "tab-closed") return "Control stopped because the controlled tab closed.";
  if (reason === "target-unavailable") return "Control stopped because this page cannot receive gestures.";
  return "Control stopped.";
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}
