use crate::config::AppSettings;
use crate::types::{ControlIntent, Landmark};

#[derive(Debug, Clone, PartialEq)]
pub struct GestureHintState {
    pub label: String,
    pub move_active: bool,
    pub two_finger_pose: bool,
    pub open_palm_pose: bool,
    pub closed_palm_pose: bool,
    pub hand_down_pose: bool,
    pub scroll_mode: bool,
    pub pinch_index: bool,
    pub pinch_middle: bool,
    pub pinch_index_threshold: f32,
    pub pinch_middle_threshold: f32,
    pub hold_progress: f32,
    pub thumb_index_distance: f32,
    pub thumb_middle_distance: f32,
    pub index_middle_distance: f32,
    pub index_extended: bool,
    pub middle_extended: bool,
    pub ring_extended: bool,
    pub pinky_extended: bool,
}

impl GestureHintState {
    fn no_hand() -> Self {
        Self {
            label: "no_hand".to_string(),
            move_active: false,
            two_finger_pose: false,
            open_palm_pose: false,
            closed_palm_pose: false,
            hand_down_pose: false,
            scroll_mode: false,
            pinch_index: false,
            pinch_middle: false,
            pinch_index_threshold: 0.0,
            pinch_middle_threshold: 0.0,
            hold_progress: 0.0,
            thumb_index_distance: 0.0,
            thumb_middle_distance: 0.0,
            index_middle_distance: 0.0,
            index_extended: false,
            middle_extended: false,
            ring_extended: false,
            pinky_extended: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FrameSignals {
    index_extended: bool,
    middle_extended: bool,
    ring_extended: bool,
    pinky_extended: bool,
    two_finger_pose: bool,
    open_palm_pose: bool,
    closed_palm_pose: bool,
    hand_down_pose: bool,
    pinch_index: bool,
    pinch_middle: bool,
    pinch_index_threshold: f32,
    pinch_middle_threshold: f32,
    thumb_index_distance: f32,
    thumb_middle_distance: f32,
    index_middle_distance: f32,
}

impl FrameSignals {
    fn from_landmarks(
        landmarks: &[Landmark],
        pinch_threshold: f32,
        right_pinch_threshold: f32,
    ) -> Option<Self> {
        if landmarks.len() < 21 {
            return None;
        }

        let index_extended = is_extended(landmarks[8], landmarks[6], landmarks[5]);
        let middle_extended = is_extended(landmarks[12], landmarks[10], landmarks[9]);
        let ring_extended = is_extended(landmarks[16], landmarks[14], landmarks[13]);
        let pinky_extended = is_extended(landmarks[20], landmarks[18], landmarks[17]);
        let two_finger_pose =
            index_extended && middle_extended && !ring_extended && !pinky_extended;
        let hand_scale = estimate_hand_scale(landmarks);
        let all_curled = is_curled_with_scale(landmarks[8], landmarks[5], hand_scale)
            && is_curled_with_scale(landmarks[12], landmarks[9], hand_scale)
            && is_curled_with_scale(landmarks[16], landmarks[13], hand_scale)
            && is_curled_with_scale(landmarks[20], landmarks[17], hand_scale);
        let all_not_extended =
            !index_extended && !middle_extended && !ring_extended && !pinky_extended;
        let closed_palm_pose = all_not_extended && all_curled;
        // Open palm is intentionally relaxed (ring OR pinky extended) for real camera variance.
        let open_palm_pose = index_extended
            && middle_extended
            && (ring_extended || pinky_extended)
            && !closed_palm_pose;
        let hand_down_pose = is_down(landmarks[8], landmarks[5])
            && is_down(landmarks[12], landmarks[9])
            && is_down(landmarks[16], landmarks[13])
            && is_down(landmarks[20], landmarks[17]);

        let thumb_index_distance = landmarks[4].distance(&landmarks[8]);
        let thumb_middle_distance = landmarks[4].distance(&landmarks[12]);
        let index_middle_distance = landmarks[8].distance(&landmarks[12]);
        let pinch_index_threshold = pinch_threshold.max((hand_scale * 0.34).clamp(0.028, 0.085));
        let pinch_middle_threshold =
            right_pinch_threshold.max((hand_scale * 0.37).clamp(0.030, 0.090));

        let pinch_index = thumb_index_distance < pinch_index_threshold;
        let pinch_middle = thumb_middle_distance < pinch_middle_threshold;

        Some(Self {
            index_extended,
            middle_extended,
            ring_extended,
            pinky_extended,
            two_finger_pose,
            open_palm_pose,
            closed_palm_pose,
            hand_down_pose,
            pinch_index,
            pinch_middle,
            pinch_index_threshold,
            pinch_middle_threshold,
            thumb_index_distance,
            thumb_middle_distance,
            index_middle_distance,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GestureEngine {
    settings: AppSettings,
    arm_pose_since_ms: Option<u64>,
    stop_pose_since_ms: Option<u64>,
    move_active: bool,
    prev_cursor_point: Option<Landmark>,
    pinch_index_prev: bool,
    pinch_middle_prev: bool,
    last_left_click_ms: Option<u64>,
    last_right_click_ms: Option<u64>,
}

impl GestureEngine {
    pub fn new(settings: AppSettings) -> Self {
        Self {
            settings,
            arm_pose_since_ms: None,
            stop_pose_since_ms: None,
            move_active: false,
            prev_cursor_point: None,
            pinch_index_prev: false,
            pinch_middle_prev: false,
            last_left_click_ms: None,
            last_right_click_ms: None,
        }
    }

    pub fn update_settings(&mut self, settings: AppSettings) {
        self.settings = settings;
    }

    pub fn hint(&self, ts_ms: u64, landmarks: &[Landmark]) -> GestureHintState {
        let Some(signals) = FrameSignals::from_landmarks(
            landmarks,
            self.settings.pinch_threshold,
            self.settings.right_pinch_threshold,
        ) else {
            return GestureHintState::no_hand();
        };

        let hold_progress = if self.move_active {
            1.0
        } else if signals.open_palm_pose {
            let hold_ms = self.settings.hold_to_control_ms.max(1) as f32;
            self.arm_pose_since_ms
                .map(|since| (ts_ms.saturating_sub(since) as f32 / hold_ms).clamp(0.0, 1.0))
                .unwrap_or(0.0)
        } else {
            0.0
        };

        let label = if self.move_active {
            if signals.closed_palm_pose {
                "stop_tracking_pose"
            } else if signals.pinch_index {
                "left_click_pinch"
            } else if signals.pinch_middle {
                "right_click_pinch"
            } else if signals.two_finger_pose {
                "move_cursor"
            } else if signals.open_palm_pose {
                "tracking_active_open_palm"
            } else if signals.hand_down_pose {
                "tracking_active_hand_down"
            } else {
                "tracking_active_idle"
            }
        } else if signals.open_palm_pose {
            if hold_progress >= 1.0 {
                "control_ready"
            } else {
                "arming_control_open_palm"
            }
        } else if signals.closed_palm_pose {
            "stop_tracking_pose"
        } else {
            "idle_hand"
        };

        GestureHintState {
            label: label.to_string(),
            move_active: self.move_active,
            two_finger_pose: signals.two_finger_pose,
            open_palm_pose: signals.open_palm_pose,
            closed_palm_pose: signals.closed_palm_pose,
            hand_down_pose: signals.hand_down_pose,
            scroll_mode: false,
            pinch_index: signals.pinch_index,
            pinch_middle: signals.pinch_middle,
            pinch_index_threshold: signals.pinch_index_threshold,
            pinch_middle_threshold: signals.pinch_middle_threshold,
            hold_progress,
            thumb_index_distance: signals.thumb_index_distance,
            thumb_middle_distance: signals.thumb_middle_distance,
            index_middle_distance: signals.index_middle_distance,
            index_extended: signals.index_extended,
            middle_extended: signals.middle_extended,
            ring_extended: signals.ring_extended,
            pinky_extended: signals.pinky_extended,
        }
    }

    pub fn reset_on_lost(&mut self, _ts_ms: u64) -> Vec<ControlIntent> {
        self.arm_pose_since_ms = None;
        self.stop_pose_since_ms = None;
        self.prev_cursor_point = None;
        self.pinch_index_prev = false;
        self.pinch_middle_prev = false;

        if self.move_active {
            self.move_active = false;
            vec![ControlIntent::ControlOff]
        } else {
            Vec::new()
        }
    }

    pub fn process(&mut self, ts_ms: u64, landmarks: &[Landmark]) -> Vec<ControlIntent> {
        let Some(signals) = FrameSignals::from_landmarks(
            landmarks,
            self.settings.pinch_threshold,
            self.settings.right_pinch_threshold,
        ) else {
            return Vec::new();
        };

        let mut intents = Vec::new();
        let start_pose = signals.open_palm_pose;
        let move_pose = signals.two_finger_pose;
        let stop_pose = signals.closed_palm_pose;
        let pinch_index = signals.pinch_index;
        let pinch_middle = signals.pinch_middle;

        if !self.move_active {
            if start_pose {
                if self.arm_pose_since_ms.is_none() {
                    self.arm_pose_since_ms = Some(ts_ms);
                }
                if let Some(since) = self.arm_pose_since_ms {
                    if ts_ms.saturating_sub(since) >= self.settings.hold_to_control_ms {
                        self.move_active = true;
                        intents.push(ControlIntent::ControlOn);
                    }
                }
            } else {
                self.arm_pose_since_ms = None;
            }
            self.stop_pose_since_ms = None;
            self.pinch_index_prev = pinch_index;
            self.pinch_middle_prev = pinch_middle;
            return intents;
        }

        self.arm_pose_since_ms = None;

        if stop_pose {
            const STOP_HOLD_MS: u64 = 180;
            let since = self.stop_pose_since_ms.get_or_insert(ts_ms);
            if ts_ms.saturating_sub(*since) >= STOP_HOLD_MS {
                self.move_active = false;
                self.prev_cursor_point = None;
                self.stop_pose_since_ms = None;
                intents.push(ControlIntent::ControlOff);
            }
            self.pinch_index_prev = pinch_index;
            self.pinch_middle_prev = pinch_middle;
            return intents;
        }
        self.stop_pose_since_ms = None;

        if move_pose {
            let cursor = landmarks[8];
            if let Some(prev) = self.prev_cursor_point {
                let mut dx = (cursor.x - prev.x) * self.settings.move_gain;
                let mut dy = (cursor.y - prev.y) * self.settings.move_gain;
                if dx.abs() < self.settings.deadzone {
                    dx = 0.0;
                }
                if dy.abs() < self.settings.deadzone {
                    dy = 0.0;
                }
                if dx != 0.0 || dy != 0.0 {
                    intents.push(ControlIntent::MoveDelta { dx, dy });
                }
            }
            self.prev_cursor_point = Some(cursor);
        } else {
            self.prev_cursor_point = None;
        }

        if pinch_index
            && !self.pinch_index_prev
            && can_fire(
                ts_ms,
                &mut self.last_left_click_ms,
                self.settings.click_cooldown_ms,
            )
        {
            intents.push(ControlIntent::LeftClick);
        }
        if pinch_middle
            && !self.pinch_middle_prev
            && can_fire(
                ts_ms,
                &mut self.last_right_click_ms,
                self.settings.click_cooldown_ms,
            )
        {
            intents.push(ControlIntent::RightClick);
        }

        self.pinch_index_prev = pinch_index;
        self.pinch_middle_prev = pinch_middle;
        intents
    }
}

fn can_fire(ts_ms: u64, last_ts: &mut Option<u64>, cooldown_ms: u64) -> bool {
    if let Some(prev) = *last_ts {
        if ts_ms.saturating_sub(prev) < cooldown_ms {
            return false;
        }
    }
    *last_ts = Some(ts_ms);
    true
}

fn is_extended(tip: Landmark, pip: Landmark, mcp: Landmark) -> bool {
    tip.y < pip.y && pip.y < mcp.y
}

fn is_down(tip: Landmark, mcp: Landmark) -> bool {
    tip.y > mcp.y
}

fn is_curled_with_scale(tip: Landmark, mcp: Landmark, hand_scale: f32) -> bool {
    tip.distance(&mcp) <= (hand_scale * 0.70).clamp(0.06, 0.14)
}

fn estimate_hand_scale(landmarks: &[Landmark]) -> f32 {
    let wrist_mid = landmarks[0].distance(&landmarks[9]);
    let palm_width = landmarks[5].distance(&landmarks[17]);
    wrist_mid.max(palm_width).max(0.08)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_hand() -> Vec<Landmark> {
        let mut lm = vec![
            Landmark {
                x: 0.5,
                y: 0.8,
                z: 0.0
            };
            21
        ];
        lm[5] = Landmark {
            x: 0.45,
            y: 0.6,
            z: 0.0,
        };
        lm[6] = Landmark {
            x: 0.45,
            y: 0.5,
            z: 0.0,
        };
        lm[8] = Landmark {
            x: 0.45,
            y: 0.4,
            z: 0.0,
        };
        lm[9] = Landmark {
            x: 0.5,
            y: 0.62,
            z: 0.0,
        };
        lm[10] = Landmark {
            x: 0.5,
            y: 0.52,
            z: 0.0,
        };
        lm[12] = Landmark {
            x: 0.5,
            y: 0.42,
            z: 0.0,
        };
        lm[13] = Landmark {
            x: 0.55,
            y: 0.6,
            z: 0.0,
        };
        lm[14] = Landmark {
            x: 0.55,
            y: 0.64,
            z: 0.0,
        };
        lm[16] = Landmark {
            x: 0.55,
            y: 0.68,
            z: 0.0,
        };
        lm[17] = Landmark {
            x: 0.6,
            y: 0.6,
            z: 0.0,
        };
        lm[18] = Landmark {
            x: 0.6,
            y: 0.64,
            z: 0.0,
        };
        lm[20] = Landmark {
            x: 0.6,
            y: 0.68,
            z: 0.0,
        };
        lm[4] = Landmark {
            x: 0.35,
            y: 0.45,
            z: 0.0,
        };
        lm
    }

    fn open_palm_hand() -> Vec<Landmark> {
        let mut lm = base_hand();
        lm[14] = Landmark {
            x: 0.55,
            y: 0.52,
            z: 0.0,
        };
        lm[16] = Landmark {
            x: 0.55,
            y: 0.42,
            z: 0.0,
        };
        lm[18] = Landmark {
            x: 0.6,
            y: 0.53,
            z: 0.0,
        };
        lm[20] = Landmark {
            x: 0.6,
            y: 0.44,
            z: 0.0,
        };
        lm
    }

    fn two_finger_hand() -> Vec<Landmark> {
        base_hand()
    }

    fn closed_hand() -> Vec<Landmark> {
        let mut lm = base_hand();
        lm[8] = Landmark {
            x: 0.45,
            y: 0.69,
            z: 0.0,
        };
        lm[12] = Landmark {
            x: 0.5,
            y: 0.70,
            z: 0.0,
        };
        lm[16] = Landmark {
            x: 0.55,
            y: 0.71,
            z: 0.0,
        };
        lm[20] = Landmark {
            x: 0.6,
            y: 0.72,
            z: 0.0,
        };
        lm
    }

    fn hand_down_open_hand() -> Vec<Landmark> {
        let mut lm = base_hand();
        lm[6] = Landmark {
            x: 0.45,
            y: 0.70,
            z: 0.0,
        };
        lm[8] = Landmark {
            x: 0.45,
            y: 0.80,
            z: 0.0,
        };
        lm[10] = Landmark {
            x: 0.50,
            y: 0.72,
            z: 0.0,
        };
        lm[12] = Landmark {
            x: 0.50,
            y: 0.82,
            z: 0.0,
        };
        lm[14] = Landmark {
            x: 0.55,
            y: 0.70,
            z: 0.0,
        };
        lm[16] = Landmark {
            x: 0.55,
            y: 0.80,
            z: 0.0,
        };
        lm[18] = Landmark {
            x: 0.60,
            y: 0.70,
            z: 0.0,
        };
        lm[20] = Landmark {
            x: 0.60,
            y: 0.80,
            z: 0.0,
        };
        lm
    }

    #[test]
    fn open_palm_hold_enables_control() {
        let mut g = GestureEngine::new(AppSettings::default());
        let lm = open_palm_hand();
        assert!(g.process(0, &lm).is_empty());
        let out = g.process(100, &lm);
        assert!(out.iter().any(|i| matches!(i, ControlIntent::ControlOn)));
    }

    #[test]
    fn two_finger_does_not_start_control_by_itself() {
        let mut g = GestureEngine::new(AppSettings::default());
        let lm = two_finger_hand();
        let _ = g.process(0, &lm);
        let out = g.process(120, &lm);
        assert!(!out.iter().any(|i| matches!(i, ControlIntent::ControlOn)));
    }

    #[test]
    fn index_pinch_generates_left_click() {
        let mut g = GestureEngine::new(AppSettings::default());
        let mut lm = open_palm_hand();
        let _ = g.process(0, &lm);
        let _ = g.process(100, &lm);
        lm = two_finger_hand();
        lm[4] = Landmark {
            x: lm[8].x + 0.002,
            y: lm[8].y + 0.002,
            z: 0.0,
        };
        let out = g.process(200, &lm);
        assert!(out.iter().any(|i| matches!(i, ControlIntent::LeftClick)));
    }

    #[test]
    fn middle_pinch_generates_right_click() {
        let mut g = GestureEngine::new(AppSettings::default());
        let mut lm = open_palm_hand();
        let _ = g.process(0, &lm);
        let _ = g.process(100, &lm);
        lm = two_finger_hand();
        lm[4] = Landmark {
            x: lm[12].x + 0.002,
            y: lm[12].y + 0.001,
            z: 0.0,
        };
        let out = g.process(220, &lm);
        assert!(out.iter().any(|i| matches!(i, ControlIntent::RightClick)));
    }

    #[test]
    fn two_finger_slide_generates_cursor_move() {
        let mut g = GestureEngine::new(AppSettings::default());
        let mut lm = open_palm_hand();
        let _ = g.process(0, &lm);
        let _ = g.process(100, &lm);
        lm = two_finger_hand();
        let _ = g.process(150, &lm);
        lm[8].x += 0.03;
        lm[8].y -= 0.02;
        let out = g.process(150, &lm);
        assert!(out
            .iter()
            .any(|i| matches!(i, ControlIntent::MoveDelta { .. })));
        assert!(!out
            .iter()
            .any(|i| matches!(i, ControlIntent::Scroll { .. })));
    }

    #[test]
    fn closed_palm_stops_control() {
        let mut g = GestureEngine::new(AppSettings::default());
        let lm_open = open_palm_hand();
        let _ = g.process(0, &lm_open);
        let out_on = g.process(100, &lm_open);
        assert!(out_on.iter().any(|i| matches!(i, ControlIntent::ControlOn)));
        let lm_closed = closed_hand();
        let h = g.hint(140, &lm_closed);
        assert!(h.closed_palm_pose);
        let _ = g.process(140, &lm_closed);
        let out = g.process(330, &lm_closed);
        assert!(out.iter().any(|i| matches!(i, ControlIntent::ControlOff)));
    }

    #[test]
    fn brief_closed_palm_does_not_stop_control() {
        let mut g = GestureEngine::new(AppSettings::default());
        let lm_open = open_palm_hand();
        let _ = g.process(0, &lm_open);
        let out_on = g.process(100, &lm_open);
        assert!(out_on.iter().any(|i| matches!(i, ControlIntent::ControlOn)));

        let lm_closed = closed_hand();
        let out = g.process(160, &lm_closed);
        assert!(!out.iter().any(|i| matches!(i, ControlIntent::ControlOff)));
    }

    #[test]
    fn hand_down_without_fist_does_not_stop_control() {
        let mut g = GestureEngine::new(AppSettings::default());
        let lm_open = open_palm_hand();
        let _ = g.process(0, &lm_open);
        let out_on = g.process(100, &lm_open);
        assert!(out_on.iter().any(|i| matches!(i, ControlIntent::ControlOn)));

        let lm_down = hand_down_open_hand();
        let out = g.process(140, &lm_down);
        assert!(!out.iter().any(|i| matches!(i, ControlIntent::ControlOff)));
    }

    #[test]
    fn hint_reports_arming_and_motion_state() {
        let mut g = GestureEngine::new(AppSettings::default());
        let lm = open_palm_hand();
        let _ = g.process(0, &lm);
        let h = g.hint(30, &lm);
        assert_eq!(h.label, "arming_control_open_palm");
        assert!(!h.move_active);
        let _ = g.process(100, &lm); // ControlOn
        let lm_move = two_finger_hand();
        let h2 = g.hint(120, &lm_move);
        assert_eq!(h2.label, "move_cursor");
        assert!(h2.move_active);
    }

    #[test]
    fn relaxed_open_palm_with_three_fingers_starts_control() {
        let mut g = GestureEngine::new(AppSettings::default());
        let mut lm = open_palm_hand();
        // Pinky bent but ring extended should still count as open palm start pose.
        lm[18] = Landmark {
            x: 0.6,
            y: 0.64,
            z: 0.0,
        };
        lm[20] = Landmark {
            x: 0.6,
            y: 0.69,
            z: 0.0,
        };
        let _ = g.process(0, &lm);
        let out = g.process(100, &lm);
        assert!(out.iter().any(|i| matches!(i, ControlIntent::ControlOn)));
    }
}
