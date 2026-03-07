# Comfort Gesture Mouse

Desktop app that uses webcam hand tracking (MediaPipe) to control the mouse with minimal gestures.

## Current Gesture Map

- Open palm (hold briefly): enable control (`control_on`)
- Index + middle fingers: move cursor
- Thumb + index pinch: left click
- Thumb + middle pinch: right click
- Closed fist (hold briefly): disable control (`control_off`)

## Quick Start (Windows)

```powershell
python -m pip install -r vision/requirements.txt
cargo run -p gesture-mouse-desktop
```

In the app:

1. Click `Start`
2. Wait for camera/model warmup
3. Use gestures above to control pointer

## Tech Stack

- Rust workspace (`tracking-core` + Tauri desktop app)
- Python sidecar for MediaPipe + OpenCV (`vision/mediapipe_tracker.py`)
- Web UI served by Tauri (`ui/index.html`)

## Notes

- Runtime diagnostics/events are written to local app data as `runtime-events.jsonl`.
- Default mouse injection is safety-gated in settings.

## License

MIT. See [LICENSE](LICENSE).
