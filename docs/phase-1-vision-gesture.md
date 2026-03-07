# Phase 1: Vision & Gesture Engine (Implemented Baseline)

## What exists now
- Rust workspace with `tracking-sidecar` binary.
- Versioned NDJSON IPC envelopes:
  - `tracking_event`
  - `control_intent`
  - `health_event`
- One-Euro smoothing for 21 landmarks.
- Gesture FSM with debounce windows:
  - two-finger hold -> `move_active_on/off`
  - pinch begin -> `click`
  - second pinch within window -> `double_click`
  - open-palm hold -> `pause_request`
- Lost tracking timeout and reset behavior.
- Trace output support (`--trace-file`).
- Unit tests for smoothing and gesture transitions.

## Run
```bash
cargo run -p tracking-sidecar -- --max-frames 300
```

## Useful flags
- `--profile <path>`: JSON config override.
- `--trace-file <path>`: write all IPC records to NDJSON file.
- `--fps`, `--width`, `--height`, `--camera-index`: runtime options.

## Current limitation
- Tracker module is currently a deterministic mock generator (`MockTracker`) to validate pipeline behavior.

## Next code step for full Phase 1 fidelity
- Replace `MockTracker` in `src/pipeline.rs` with ONNX-backed inference:
  - palm detector + hand landmark models,
  - ROI tracking and periodic detector refresh,
  - real confidence from model outputs.
