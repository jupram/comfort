# Stage 3: Core Gestures

## Goal
Implement minimal mouse replacement gesture set.

## Scope
- Hold-to-control activation.
- Relative pointer movement.
- Left click via thumb-index pinch.
- Right click via thumb-middle pinch.
- Scroll via two-finger vertical slide.
- Gesture debounce/cooldown and reset-on-tracking-loss behavior.

## Exit Criteria
- Gesture FSM emits correct intents with debounce/cooldown.
- False positives are reduced with hysteresis and confidence gating.
- Manual tests validate practical mouse replacement behavior.

## Runtime Event Stream
- `vision_status`: lifecycle states (`loading_model`, `loading_camera`, `ready`, `running`, `paused`, `stopped`).
- `tracking_status`: tracker confidence state (`tracking`, `low_confidence`, `lost`).
- `tracking_frame`: filtered landmarks and confidence.
- `camera_preview`: JPEG preview frame with landmark overlay.
- `gesture_debug`: emitted control intents (`move_delta`, `left_click`, `right_click`, `scroll`, `control_on`, `control_off`, `paused`).
- `health_metrics`: fps and latency health snapshot.

## Implemented Defaults
- Hold-to-control debounce: `hold_to_control_ms` (default `70`).
- Left-click pinch threshold: `pinch_threshold` (default `0.035`).
- Right-click pinch threshold: `right_pinch_threshold` (default `0.040`).
- Click cooldown: `click_cooldown_ms` (default `160`).
- Scroll mode trigger: index-middle tip distance below `scroll_mode_threshold` (default `0.045`).
- Scroll gain/deadzone: `scroll_gain=28.0`, `scroll_deadzone=0.6`.
