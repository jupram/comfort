# Stage 2: Vision Integration

## Goal
Replace mock landmarks with real hand landmarks from webcam.

## Scope
- Camera capture and camera selection.
- MediaPipe hand landmark inference via local Python sidecar (`vision/mediapipe_tracker.py`).
- Confidence handling and lost-tracking transitions.
- Landmark smoothing integrated into runtime.

## Exit Criteria
- Stable landmark stream at target webcam FPS.
- Tracking status transitions reflect confidence and loss behavior.
- Diagnostics surface confidence and dropped-frame signals.
- UI receives `vision_status` lifecycle events (`loading_model`, `loading_camera`, `ready`) during startup.

## Local Setup
- Install sidecar dependencies:
  - `python -m pip install -r vision/requirements.txt`
- Start the desktop app:
  - `cargo run -p gesture-mouse-desktop`
