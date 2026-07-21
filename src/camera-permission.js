import "./camera-permission.css";

const elements = {
  allowButton: document.querySelector("#allow-camera"),
  closeButton: document.querySelector("#close-page"),
  placeholder: document.querySelector("#preview-placeholder"),
  preview: document.querySelector("#permission-preview"),
  status: document.querySelector("#permission-status"),
};

let stream = null;

elements.allowButton.addEventListener("click", () => {
  void requestCamera();
});

elements.closeButton.addEventListener("click", () => {
  window.close();
});

window.addEventListener("pagehide", stopPreview);

void showCurrentPermission();

async function showCurrentPermission() {
  if (!navigator.permissions?.query) return;

  try {
    const permission = await navigator.permissions.query({ name: "camera" });
    if (permission.state === "granted") {
      setStatus("Camera access is already allowed. Test it below, or return to Comfort and enable the camera.", "success");
    } else if (permission.state === "denied") {
      setStatus("Camera access is blocked. Use the camera icon in the address bar to allow it, then retry.", "error");
    }
  } catch {
    // Some Chromium versions do not expose camera through Permissions API.
  }
}

async function requestCamera() {
  elements.allowButton.disabled = true;
  setStatus("Waiting for camera permission...", "info");

  try {
    stopPreview();
    stream = await navigator.mediaDevices.getUserMedia({
      audio: false,
      video: {
        facingMode: "user",
        width: { ideal: 640 },
        height: { ideal: 480 },
      },
    });

    elements.preview.srcObject = stream;
    await elements.preview.play();
    elements.placeholder.hidden = true;
    elements.allowButton.textContent = "Camera allowed";
    elements.allowButton.disabled = true;
    elements.closeButton.hidden = false;
    setStatus("Camera access is working. Return to Comfort and click Enable camera.", "success");
  } catch (error) {
    elements.allowButton.disabled = false;
    if (error?.name === "NotAllowedError") {
      setStatus("The browser still has camera access blocked. Click the camera icon in the address bar, choose Allow, and retry.", "error");
    } else if (error?.name === "NotFoundError") {
      setStatus("No camera was found on this device.", "error");
    } else if (error?.name === "NotReadableError") {
      setStatus("The camera is busy or unavailable. Close other apps using it and retry.", "error");
    } else {
      setStatus(`Could not open the camera: ${error?.message || "Unknown error"}`, "error");
    }
  }
}

function stopPreview() {
  stream?.getTracks().forEach((track) => track.stop());
  stream = null;
}

function setStatus(message, kind) {
  elements.status.textContent = message;
  elements.status.dataset.kind = kind;
}
