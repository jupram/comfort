use crate::config::{AppSettings, VisionBackend};
use crate::types::Landmark;
use serde::Deserialize;
use std::f32::consts::PI;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use tracing::{debug, warn};

const MAX_TRACKER_LINE_BYTES: usize = 2 * 1024 * 1024;
const MAX_PREVIEW_BASE64_BYTES: usize = 2 * 1024 * 1024;

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

#[derive(Default)]
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
        let active_phase = (35..190).contains(&phase);
        if active_phase && (click_cycle == 90 || click_cycle == 91) {
            lm[4] = Landmark {
                x: lm[8].x + 0.001,
                y: lm[8].y + 0.001,
                z: 0.0,
            };
        }

        // Periodic right click pinch.
        if active_phase && (click_cycle == 140 || click_cycle == 141) {
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

impl PythonPacket {
    fn into_tracker_packet(self) -> TrackerPacket {
        TrackerPacket {
            frame_id: self.frame_id,
            ts_ms: self.ts_ms,
            confidence: sanitize_confidence(self.confidence),
            landmarks: sanitize_landmarks(self.landmarks),
            preview_jpeg_base64: sanitize_preview(self.preview_jpeg_base64),
            dropped: false,
        }
    }
}

pub struct PythonMediapipeTracker {
    latest: Arc<Mutex<Option<TrackerPacket>>>,
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

        let latest = Arc::new(Mutex::new(None));
        let latest_writer = Arc::clone(&latest);

        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            if let Err(e) =
                read_python_tracker_stream(&mut reader, &latest_writer, MAX_TRACKER_LINE_BYTES)
            {
                warn!("python tracker stream read failed: {e}");
            }
        });

        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for raw in reader.lines().map_while(Result::ok) {
                if !raw.trim().is_empty() {
                    warn!("python tracker: {raw}");
                }
            }
        });

        Ok(Self {
            latest,
            child,
            frame_id: 0,
            fallback: MockVisionTracker::new(),
            child_exited: false,
        })
    }
}

impl VisionTracker for PythonMediapipeTracker {
    fn next(&mut self, ts_ms: u64) -> TrackerPacket {
        let latest = self.latest.lock().ok().and_then(|mut latest| latest.take());
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
        if !self.child_exited && matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

fn sanitize_confidence(confidence: f32) -> f32 {
    if confidence.is_finite() {
        confidence.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn sanitize_landmarks(mut landmarks: Vec<Landmark>) -> Option<Vec<Landmark>> {
    if landmarks.len() != 21
        || landmarks
            .iter()
            .any(|lm| !lm.x.is_finite() || !lm.y.is_finite() || !lm.z.is_finite())
    {
        return None;
    }
    for lm in &mut landmarks {
        lm.x = lm.x.clamp(0.0, 1.0);
        lm.y = lm.y.clamp(0.0, 1.0);
        lm.z = lm.z.clamp(-1.0, 1.0);
    }
    Some(landmarks)
}

fn sanitize_preview(preview: Option<String>) -> Option<String> {
    let preview = preview?;
    if preview.len() > MAX_PREVIEW_BASE64_BYTES
        || !preview
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
    {
        return None;
    }
    Some(preview)
}

fn read_python_tracker_stream<R: BufRead>(
    reader: &mut R,
    latest_writer: &Arc<Mutex<Option<TrackerPacket>>>,
    max_line_len: usize,
) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity(64 * 1024);
    while read_bounded_line(reader, &mut raw, max_line_len)? {
        if raw.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<PythonPacket>(&raw) {
            Ok(packet) => {
                let packet = packet.into_tracker_packet();
                let mut latest = latest_writer.lock().map_err(|_| {
                    std::io::Error::other("python tracker latest packet lock poisoned")
                })?;
                *latest = Some(packet);
            }
            Err(e) => {
                debug!("python tracker JSON parse failed: {e}");
            }
        }
    }
    Ok(())
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max_len: usize,
) -> std::io::Result<bool> {
    buf.clear();
    loop {
        let (take_len, has_newline, is_eof) = {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                (0, false, true)
            } else {
                let newline_pos = available.iter().position(|&b| b == b'\n');
                let take_len = newline_pos.map_or(available.len(), |pos| pos + 1);
                if buf.len().saturating_add(take_len) > max_len {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "python tracker line exceeded maximum length",
                    ));
                }
                buf.extend_from_slice(&available[..take_len]);
                (take_len, newline_pos.is_some(), false)
            }
        };

        if is_eof {
            if buf.is_empty() {
                return Ok(false);
            }
            break;
        }

        reader.consume(take_len);
        if has_newline {
            break;
        }
    }

    while matches!(buf.last(), Some(b'\n' | b'\r')) {
        buf.pop();
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn python_packet_sanitizes_confidence_landmarks_and_preview() {
        let landmarks = vec![
            Landmark {
                x: 1.5,
                y: -0.5,
                z: 2.0,
            };
            21
        ];
        let packet = PythonPacket {
            frame_id: 7,
            ts_ms: 99,
            confidence: 1.5,
            landmarks,
            preview_jpeg_base64: Some("YWJjZA==".to_string()),
        }
        .into_tracker_packet();

        assert_eq!(packet.confidence, 1.0);
        let landmarks = packet.landmarks.expect("valid landmarks");
        assert_eq!(landmarks[0].x, 1.0);
        assert_eq!(landmarks[0].y, 0.0);
        assert_eq!(landmarks[0].z, 1.0);
        assert_eq!(packet.preview_jpeg_base64.as_deref(), Some("YWJjZA=="));
    }

    #[test]
    fn python_packet_rejects_non_finite_landmarks_and_invalid_preview() {
        let mut landmarks = vec![
            Landmark {
                x: 0.5,
                y: 0.5,
                z: 0.0,
            };
            21
        ];
        landmarks[3].x = f32::NAN;

        let packet = PythonPacket {
            frame_id: 7,
            ts_ms: 99,
            confidence: f32::NAN,
            landmarks,
            preview_jpeg_base64: Some("not a data url".to_string()),
        }
        .into_tracker_packet();

        assert_eq!(packet.confidence, 0.0);
        assert!(packet.landmarks.is_none());
        assert!(packet.preview_jpeg_base64.is_none());
    }

    #[test]
    fn bounded_line_reader_rejects_oversized_lines() {
        let mut reader = Cursor::new(b"abcdef\n".to_vec());
        let mut buf = Vec::new();
        let err = read_bounded_line(&mut reader, &mut buf, 4).expect_err("line too large");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn bounded_line_reader_reads_line_without_newline_at_eof() {
        let mut reader = Cursor::new(b"abc".to_vec());
        let mut buf = Vec::new();
        let has_line = read_bounded_line(&mut reader, &mut buf, 4).expect("read");
        assert!(has_line);
        assert_eq!(buf, b"abc");
    }

    #[test]
    fn bounded_line_reader_reuses_buffer_for_multiple_lines() {
        let mut reader = Cursor::new(b"abc\ndef\n".to_vec());
        let mut buf = Vec::new();

        assert!(read_bounded_line(&mut reader, &mut buf, 8).expect("first"));
        assert_eq!(buf, b"abc");
        assert!(read_bounded_line(&mut reader, &mut buf, 8).expect("second"));
        assert_eq!(buf, b"def");
        assert!(!read_bounded_line(&mut reader, &mut buf, 8).expect("eof"));
    }

    #[test]
    fn python_tracker_stream_keeps_latest_valid_packet() {
        let mut reader = Cursor::new(
            br#"{"frame_id":1,"ts_ms":10,"confidence":0.2,"landmarks":[]}
{"frame_id":2,"ts_ms":20,"confidence":0.9,"landmarks":[]}
"#
            .to_vec(),
        );
        let latest = Arc::new(Mutex::new(None));

        read_python_tracker_stream(&mut reader, &latest, 1024).expect("stream");

        let packet = latest.lock().expect("latest").clone().expect("packet");
        assert_eq!(packet.frame_id, 2);
        assert_eq!(packet.ts_ms, 20);
        assert_eq!(packet.confidence, 0.9);
        assert!(packet.landmarks.is_none());
    }

    #[test]
    fn python_tracker_stream_ignores_malformed_lines() {
        let mut reader = Cursor::new(
            br#"not json
{"frame_id":3,"ts_ms":30,"confidence":0.4,"landmarks":[]}
"#
            .to_vec(),
        );
        let latest = Arc::new(Mutex::new(None));

        read_python_tracker_stream(&mut reader, &latest, 1024).expect("stream");

        let packet = latest.lock().expect("latest").clone().expect("packet");
        assert_eq!(packet.frame_id, 3);
        assert_eq!(packet.confidence, 0.4);
    }

    #[test]
    fn python_tracker_stream_returns_error_on_oversized_line() {
        let mut reader = Cursor::new(b"abcdef\n".to_vec());
        let latest = Arc::new(Mutex::new(None));

        let err = read_python_tracker_stream(&mut reader, &latest, 4).expect_err("oversized");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(latest.lock().expect("latest").is_none());
    }
}
