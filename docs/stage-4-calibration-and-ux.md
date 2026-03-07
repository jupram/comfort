# Stage 4: Calibration and UX

## Goal
Improve comfort and reliability with per-user calibration and settings UX.

## Scope
- Guided calibration flow in desktop UI.
- Per-user thresholds and sensitivity tuning with named calibration profiles.
- Recalibration and restore-default actions.
- Better diagnostics in settings window.

## Exit Criteria
- New user can complete calibration in ~30 seconds.
- Saved profile improves control consistency across sessions.

## Implemented
- `CalibrationProfile` added to settings schema:
  - `comfort`, `balanced`, `responsive`.
  - Persisted with `calibration_profile` and `calibrated_at_ms`.
- Calibration backend:
  - `run_calibration(request?)` now applies profile baselines plus optional per-user overrides.
  - Override fields include movement sensitivity, deadzone, hold timing, pinch thresholds, click cooldown, and confidence hysteresis.
  - Values are clamped to safe ranges before save/apply.
- Restore defaults:
  - `reset_settings` command resets persisted config to `AppSettings::default()` and hot-applies runtime changes.
- Settings UX:
  - Full editable settings panel with save/load and key runtime controls.
  - Includes input safety toggles and camera/runtime fields.
- Guided calibration UX:
  - Step-based flow:
    1. Open palm hold.
    2. Two-finger movement.
    3. Left/right click pinch checks.
    4. Closed-fist stop.
  - Collects runtime metrics during flow (confidence, movement, pinch distances, stability/jitter).
  - Generates a suggested calibration request and applies it through `run_calibration`.
- Diagnostics:
  - Live runtime status, pose hint, health metrics, event counters, and event log in one screen.
