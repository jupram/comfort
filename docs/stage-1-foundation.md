# Stage 1: Foundation

## Goal
Ship a runnable Windows desktop shell with mock tracking and safe input handling.

## Scope
- Tauri + Rust app shell with system tray controls.
- Settings persistence with versioned schema.
- Internal runtime pipeline: mock tracking -> control intents -> input driver.
- Safe-mode input driver (default on) that avoids accidental clicks/scroll.
- Diagnostics events and health metrics.

## Out of Scope
- Real webcam tracking.
- MediaPipe integration.
- Production gesture detection.

## Exit Criteria
- App launches and exposes tray menu actions: start, stop, pause, open settings, quit.
- Mock runtime emits `tracking_status`, `gesture_debug`, and `health_metrics` events.
- Settings load/save works via commands and persisted JSON profile.
- Input driver honors `safe_mode` and `input_injection_enabled`.

## Acceptance Checks
- Start/stop repeatedly without stuck state.
- Safe mode prevents click and wheel injection.
- Optional bounded movement injection works when enabled in settings.

