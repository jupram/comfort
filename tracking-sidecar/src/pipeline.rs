use crate::config::Cli;
use crate::types::Landmark;
use std::f32::consts::PI;

pub struct FramePacket {
    pub frame_id: u64,
    pub ts_ms: u64,
    pub confidence: f32,
    pub landmarks: Option<Vec<Landmark>>,
}

pub trait Tracker {
    fn next(&mut self, cli: &Cli, frame_id: u64, ts_ms: u64) -> FramePacket;
}

pub struct MockTracker;

impl MockTracker {
    pub fn new() -> Self {
        Self
    }
}

impl Tracker for MockTracker {
    fn next(&mut self, _cli: &Cli, frame_id: u64, ts_ms: u64) -> FramePacket {
        if frame_id % 450 >= 430 {
            return FramePacket {
                frame_id,
                ts_ms,
                confidence: 0.2,
                landmarks: None,
            };
        }

        let t = frame_id as f32 / 30.0;
        let cx = 0.5 + 0.03 * (2.0 * PI * 0.25 * t).sin();
        let cy = 0.55 + 0.02 * (2.0 * PI * 0.20 * t).cos();

        let mut lm = vec![
            Landmark {
                x: cx,
                y: cy,
                z: 0.0
            };
            21
        ];
        lm[5] = Landmark {
            x: cx - 0.04,
            y: cy - 0.02,
            z: 0.0,
        };
        lm[6] = Landmark {
            x: cx - 0.04,
            y: cy - 0.12,
            z: 0.0,
        };
        lm[8] = Landmark {
            x: cx - 0.04 + 0.01 * (2.0 * PI * 1.0 * t).sin(),
            y: cy - 0.22 + 0.006 * (2.0 * PI * 1.0 * t).cos(),
            z: 0.0,
        };

        lm[9] = Landmark {
            x: cx + 0.01,
            y: cy - 0.02,
            z: 0.0,
        };
        lm[10] = Landmark {
            x: cx + 0.01,
            y: cy - 0.12,
            z: 0.0,
        };
        lm[12] = Landmark {
            x: cx + 0.01,
            y: cy - 0.22,
            z: 0.0,
        };

        lm[13] = Landmark {
            x: cx + 0.05,
            y: cy - 0.01,
            z: 0.0,
        };
        lm[14] = Landmark {
            x: cx + 0.05,
            y: cy + 0.03,
            z: 0.0,
        };
        lm[16] = Landmark {
            x: cx + 0.05,
            y: cy + 0.07,
            z: 0.0,
        };

        lm[17] = Landmark {
            x: cx + 0.08,
            y: cy,
            z: 0.0,
        };
        lm[18] = Landmark {
            x: cx + 0.08,
            y: cy + 0.04,
            z: 0.0,
        };
        lm[20] = Landmark {
            x: cx + 0.08,
            y: cy + 0.08,
            z: 0.0,
        };

        let pinch_cycle = frame_id % 180;
        if pinch_cycle == 40 || pinch_cycle == 41 {
            lm[4] = Landmark {
                x: lm[8].x + 0.002,
                y: lm[8].y + 0.002,
                z: 0.0,
            };
        } else if pinch_cycle == 56 || pinch_cycle == 57 {
            lm[4] = Landmark {
                x: lm[8].x + 0.003,
                y: lm[8].y + 0.001,
                z: 0.0,
            };
        } else {
            lm[4] = Landmark {
                x: cx - 0.12,
                y: cy - 0.16,
                z: 0.0,
            };
        }

        if frame_id % 600 > 540 && frame_id % 600 < 575 {
            lm[13] = Landmark {
                x: cx + 0.05,
                y: cy - 0.02,
                z: 0.0,
            };
            lm[14] = Landmark {
                x: cx + 0.05,
                y: cy - 0.12,
                z: 0.0,
            };
            lm[16] = Landmark {
                x: cx + 0.05,
                y: cy - 0.22,
                z: 0.0,
            };
            lm[17] = Landmark {
                x: cx + 0.08,
                y: cy - 0.02,
                z: 0.0,
            };
            lm[18] = Landmark {
                x: cx + 0.08,
                y: cy - 0.12,
                z: 0.0,
            };
            lm[20] = Landmark {
                x: cx + 0.08,
                y: cy - 0.22,
                z: 0.0,
            };
            lm[4] = Landmark {
                x: cx - 0.22,
                y: cy - 0.16,
                z: 0.0,
            };
        }

        FramePacket {
            frame_id,
            ts_ms,
            confidence: 0.83,
            landmarks: Some(lm),
        }
    }
}
