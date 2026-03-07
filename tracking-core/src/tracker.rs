use crate::config::{AppSettings, VisionBackend};
use crate::types::Landmark;
use serde::Deserialize;
use std::f32::consts::PI;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct TrackerPacket {
    pub frame_id: u64,
    pub ts_ms: u64,
    pub confidence: f32,
    pub landmarks: Option<Vec<Landmark>>,
    pub preview_jpeg_base64: Option<String>,
    pub dropped: bool,
}

pub trait VisionTracker: Send {
    fn next(&mut self, ts_ms: u64) -> TrackerPacket;
}

pub fn create_tracker(settings: &AppSettings) -> Box<dyn VisionTracker> {
    match settings.vision_backend {
        VisionBackend::Mock => Box::new(MockVisionTracker::new()),
        VisionBackend::PythonMediapipe => {
            match PythonMediapipeTracker::new(settings)
                .map(|x| Box::new(x) as Box<dyn VisionTracker>)
            {
                Ok(tracker) => tracker,
                Err(e) => {
                    warn!("python mediapipe tracker unavailable, using mock tracker: {e}");
                    Box::new(MockVisionTracker::new())
                }
            }
        }
    }
}

pub struct MockVisionTracker {
    frame_id: u64,
}

impl MockVisionTracker {
    pub fn new() -> Self {
        Self { frame_id: 0 }
    }
}

impl VisionTracker for MockVisionTracker {
    fn next(&mut self, ts_ms: u64) -> TrackerPacket {
        let frame_id = self.frame_id;
        self.frame_id += 1;

        if frame_id % 450 >= 430 {
            return TrackerPacket {
                frame_id,
                ts_ms,
                confidence: 0.2,
                landmarks: None,
                preview_jpeg_base64: None,
                dropped: true,
            };
        }

        let t = frame_id as f32 / 30.0;
        let cx = 0.5 + 0.03 * (2.0 * PI * 0.25 * t).sin();
        let cy = 0.55 + 0.02 * (2.0 * PI * 0.20 * t).cos();
        let mut lm = vec![
            Landmark {
                x: cx,
                y: cy,
                z: 0.0,
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
        lm[4] = Landmark {
            x: cx - 0.12,
            y: cy - 0.16,
            z: 0.0,
        };
        lm[8] = Landmark {
            x: cx - 0.04 + 0.01 * (2.0 * PI * t).sin(),
            y: cy - 0.22 + 0.006 * (2.0 * PI * t).cos(),
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

        let phase = frame_id % 240;
        // Start pose: open palm for first frames.
        if phase < 35 {
            lm[14] = Landmark {
                x: cx + 0.05,
                y: cy - 0.11,
                z: 0.0,
            };
            lm[16] = Landmark {
                x: cx + 0.05,
                y: cy - 0.21,
                z: 0.0,
            };
            lm[18] = Landmark {
                x: cx + 0.08,
                y: cy - 0.11,
                z: 0.0,
            };
            lm[20] = Landmark {
                x: cx + 0.08,
                y: cy - 0.21,
                z: 0.0,
            };
        }

        // Stop pose: closed/down hand near end of cycle.
        if phase >= 190 {
            lm[8] = Landmark {
                x: cx - 0.03,
                y: cy + 0.08,
                z: 0.0,
            };
            lm[12] = Landmark {
                x: cx + 0.01,
                y: cy + 0.09,
                z: 0.0,
            };
            lm[16] = Landmark {
                x: cx + 0.05,
                y: cy + 0.10,
                z: 0.0,
            };
            lm[20] = Landmark {
                x: cx + 0.08,
                y: cy + 0.11,
                z: 0.0,
            };
        }

        // Periodic left click pinch.
        let click_cycle = frame_id % 220;
        if phase >= 35 && phase < 190 && (click_cycle == 90 || click_cycle == 91) {
            lm[4] = Landmark {
                x: lm[8].x + 0.001,
                y: lm[8].y + 0.001,
                z: 0.0,
            };
        }

        // Periodic right click pinch.
        if phase >= 35 && phase < 190 && (click_cycle == 140 || click_cycle == 141) {
            lm[4] = Landmark {
                x: lm[12].x + 0.001,
                y: lm[12].y + 0.001,
                z: 0.0,
            };
        }
        TrackerPacket {
            frame_id,
            ts_ms,
            confidence: 0.83,
            landmarks: Some(lm),
            preview_jpeg_base64: None,
            dropped: false,
        }
    }
}

#[derive(Debug, Deserialize)]
struct PythonPacket {
    #[serde(default)]
    frame_id: u64,
    ts_ms: u64,
    confidence: f32,
    #[serde(default)]
    landmarks: Vec<Landmark>,
    #[serde(default)]
    preview_jpeg_base64: Option<String>,
}

pub struct PythonMediapipeTracker {
    rx: Receiver<TrackerPacket>,
    child: Child,
    frame_id: u64,
    fallback: MockVisionTracker,
    child_exited: bool,
}

impl PythonMediapipeTracker {
    pub fn new(settings: &AppSettings) -> anyhow::Result<Self> {
        let script = settings
            .mediapipe_script_path
            .clone()
            .unwrap_or_else(|| "vision/mediapipe_tracker.py".to_string());

        let mut child = Command::new(&settings.python_executable)
            .arg(script)
            .arg("--camera-index")
            .arg(settings.camera_index.to_string())
            .arg("--width")
            .arg(settings.camera_width.to_string())
            .arg("--height")
            .arg(settings.camera_height.to_string())
            .arg("--fps")
            .arg(settings.camera_fps.to_string())
            .arg("--preview-every")
            .arg("3")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("python tracker stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("python tracker stderr unavailable"))?;

        let (tx, rx) = mpsc::channel::<TrackerPacket>();

        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(raw) = line else {
                    break;
                };
                if raw.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<PythonPacket>(&raw) {
                    Ok(packet) => {
                        let _ = tx.send(TrackerPacket {
                            frame_id: packet.frame_id,
                            ts_ms: packet.ts_ms,
                            confidence: packet.confidence,
                            landmarks: if packet.landmarks.len() == 21 {
                                Some(packet.landmarks)
                            } else {
                                None
                            },
                            preview_jpeg_base64: packet.preview_jpeg_base64,
                            dropped: false,
                        });
                    }
                    Err(e) => {
                        debug!("python tracker JSON parse failed: {e}");
                    }
                }
            }
        });

        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(raw) = line {
                    if !raw.trim().is_empty() {
                        warn!("python tracker: {raw}");
                    }
                }
            }
        });

        Ok(Self {
            rx,
            child,
            frame_id: 0,
            fallback: MockVisionTracker::new(),
            child_exited: false,
        })
    }
}

impl VisionTracker for PythonMediapipeTracker {
    fn next(&mut self, ts_ms: u64) -> TrackerPacket {
        let mut latest = None;
        while let Ok(packet) = self.rx.try_recv() {
            latest = Some(packet);
        }
        if let Some(packet) = latest {
            self.frame_id = packet.frame_id.saturating_add(1);
            packet
        } else {
            if !self.child_exited {
                if let Ok(Some(status)) = self.child.try_wait() {
                    self.child_exited = true;
                    warn!("python tracker exited ({status}), falling back to mock tracker");
                }
            }
            if self.child_exited {
                return self.fallback.next(ts_ms);
            }
            let frame_id = self.frame_id;
            self.frame_id = self.frame_id.saturating_add(1);
            TrackerPacket {
                frame_id,
                ts_ms,
                confidence: 0.0,
                landmarks: None,
                preview_jpeg_base64: None,
                dropped: true,
            }
        }
    }
}

impl Drop for PythonMediapipeTracker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
