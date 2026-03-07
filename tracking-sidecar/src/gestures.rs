use crate::config::Config;
use crate::types::{ControlIntent, ControlType, Delta, Landmark};

#[derive(Debug, Clone)]
pub struct GestureEngine {
    cfg: Config,
    move_pose_since_ms: Option<u64>,
    move_active: bool,
    pinch_prev: bool,
    last_click_ts_ms: Option<u64>,
    open_palm_since_ms: Option<u64>,
    pause_latched: bool,
    prev_cursor_point: Option<Landmark>,
}

impl GestureEngine {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            move_pose_since_ms: None,
            move_active: false,
            pinch_prev: false,
            last_click_ts_ms: None,
            open_palm_since_ms: None,
            pause_latched: false,
            prev_cursor_point: None,
        }
    }

    pub fn reset_on_lost(&mut self, ts_ms: u64) -> Vec<ControlIntent> {
        self.move_pose_since_ms = None;
        self.pinch_prev = false;
        self.open_palm_since_ms = None;
        self.pause_latched = false;
        self.prev_cursor_point = None;

        if self.move_active {
            self.move_active = false;
            vec![ControlIntent {
                ts_ms,
                intent_type: ControlType::MoveActiveOff,
                delta: None,
            }]
        } else {
            Vec::new()
        }
    }

    pub fn process(&mut self, ts_ms: u64, landmarks: &[Landmark]) -> Vec<ControlIntent> {
        let mut intents = Vec::new();

        let two_finger = is_two_finger_pose(landmarks);
        let open_palm = is_open_palm(landmarks);
        let pinch = is_pinch(landmarks, self.cfg.pinch_threshold);

        if two_finger {
            if self.move_pose_since_ms.is_none() {
                self.move_pose_since_ms = Some(ts_ms);
            }
            if !self.move_active {
                if let Some(since) = self.move_pose_since_ms {
                    if ts_ms.saturating_sub(since) >= self.cfg.move_pose_hold_ms {
                        self.move_active = true;
                        intents.push(ControlIntent {
                            ts_ms,
                            intent_type: ControlType::MoveActiveOn,
                            delta: None,
                        });
                    }
                }
            }
        } else {
            self.move_pose_since_ms = None;
            if self.move_active {
                self.move_active = false;
                self.prev_cursor_point = None;
                intents.push(ControlIntent {
                    ts_ms,
                    intent_type: ControlType::MoveActiveOff,
                    delta: None,
                });
            }
        }

        if self.move_active {
            let cursor = landmarks[8];
            if let Some(prev) = self.prev_cursor_point {
                let mut dx = (cursor.x - prev.x) * self.cfg.move_gain;
                let mut dy = (cursor.y - prev.y) * self.cfg.move_gain;
                if dx.abs() < self.cfg.deadzone {
                    dx = 0.0;
                }
                if dy.abs() < self.cfg.deadzone {
                    dy = 0.0;
                }
                if dx != 0.0 || dy != 0.0 {
                    intents.push(ControlIntent {
                        ts_ms,
                        intent_type: ControlType::MoveDelta,
                        delta: Some(Delta { x: dx, y: dy }),
                    });
                }
            }
            self.prev_cursor_point = Some(cursor);
        } else {
            self.prev_cursor_point = None;
        }

        if pinch && !self.pinch_prev {
            if let Some(last) = self.last_click_ts_ms {
                if ts_ms.saturating_sub(last) <= self.cfg.double_click_window_ms {
                    intents.push(ControlIntent {
                        ts_ms,
                        intent_type: ControlType::DoubleClick,
                        delta: None,
                    });
                } else {
                    intents.push(ControlIntent {
                        ts_ms,
                        intent_type: ControlType::Click,
                        delta: None,
                    });
                }
            } else {
                intents.push(ControlIntent {
                    ts_ms,
                    intent_type: ControlType::Click,
                    delta: None,
                });
            }
            self.last_click_ts_ms = Some(ts_ms);
        }
        self.pinch_prev = pinch;

        if open_palm {
            if self.open_palm_since_ms.is_none() {
                self.open_palm_since_ms = Some(ts_ms);
            }
            if !self.pause_latched {
                if let Some(since) = self.open_palm_since_ms {
                    if ts_ms.saturating_sub(since) >= self.cfg.pause_hold_ms {
                        self.pause_latched = true;
                        intents.push(ControlIntent {
                            ts_ms,
                            intent_type: ControlType::PauseRequest,
                            delta: None,
                        });
                    }
                }
            }
        } else {
            self.open_palm_since_ms = None;
            self.pause_latched = false;
        }

        intents
    }
}

fn is_extended(tip: Landmark, pip: Landmark, mcp: Landmark) -> bool {
    tip.y < pip.y && pip.y < mcp.y
}

fn is_two_finger_pose(landmarks: &[Landmark]) -> bool {
    if landmarks.len() < 21 {
        return false;
    }
    let index_ext = is_extended(landmarks[8], landmarks[6], landmarks[5]);
    let middle_ext = is_extended(landmarks[12], landmarks[10], landmarks[9]);
    let ring_ext = is_extended(landmarks[16], landmarks[14], landmarks[13]);
    let pinky_ext = is_extended(landmarks[20], landmarks[18], landmarks[17]);
    index_ext && middle_ext && !ring_ext && !pinky_ext
}

fn is_open_palm(landmarks: &[Landmark]) -> bool {
    if landmarks.len() < 21 {
        return false;
    }
    let index_ext = is_extended(landmarks[8], landmarks[6], landmarks[5]);
    let middle_ext = is_extended(landmarks[12], landmarks[10], landmarks[9]);
    let ring_ext = is_extended(landmarks[16], landmarks[14], landmarks[13]);
    let pinky_ext = is_extended(landmarks[20], landmarks[18], landmarks[17]);
    let thumb_spread = (landmarks[4].x - landmarks[5].x).abs() > 0.05;
    index_ext && middle_ext && ring_ext && pinky_ext && thumb_spread
}

fn is_pinch(landmarks: &[Landmark], threshold: f32) -> bool {
    if landmarks.len() < 21 {
        return false;
    }
    landmarks[4].distance(&landmarks[8]) < threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ControlType;

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

    fn with_pinch(mut lm: Vec<Landmark>) -> Vec<Landmark> {
        lm[4] = Landmark {
            x: 0.451,
            y: 0.401,
            z: 0.0,
        };
        lm
    }

    fn with_open_palm(mut lm: Vec<Landmark>) -> Vec<Landmark> {
        lm[13] = Landmark {
            x: 0.55,
            y: 0.62,
            z: 0.0,
        };
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
        lm[17] = Landmark {
            x: 0.6,
            y: 0.62,
            z: 0.0,
        };
        lm[18] = Landmark {
            x: 0.6,
            y: 0.52,
            z: 0.0,
        };
        lm[20] = Landmark {
            x: 0.6,
            y: 0.42,
            z: 0.0,
        };
        lm[4] = Landmark {
            x: 0.25,
            y: 0.45,
            z: 0.0,
        };
        lm
    }

    #[test]
    fn move_activation_uses_debounce() {
        let mut eng = GestureEngine::new(Config::default());
        let lm = base_hand();
        assert!(eng.process(0, &lm).is_empty());
        let after = eng.process(80, &lm);
        assert_eq!(after[0].intent_type, ControlType::MoveActiveOn);
    }

    #[test]
    fn pinch_begin_generates_click() {
        let mut eng = GestureEngine::new(Config::default());
        let lm = base_hand();
        let _ = eng.process(100, &lm);
        let _ = eng.process(180, &lm);
        let out = eng.process(200, &with_pinch(lm.clone()));
        assert!(out.iter().any(|x| x.intent_type == ControlType::Click));
    }

    #[test]
    fn double_click_window() {
        let mut eng = GestureEngine::new(Config::default());
        let lm = base_hand();
        let _ = eng.process(100, &with_pinch(lm.clone()));
        let _ = eng.process(130, &lm);
        let out = eng.process(300, &with_pinch(lm));
        assert!(out
            .iter()
            .any(|x| x.intent_type == ControlType::DoubleClick));
    }

    #[test]
    fn open_palm_emits_pause() {
        let mut eng = GestureEngine::new(Config::default());
        let op = with_open_palm(base_hand());
        let _ = eng.process(100, &op);
        let out = eng.process(290, &op);
        assert!(out
            .iter()
            .any(|x| x.intent_type == ControlType::PauseRequest));
    }
}
