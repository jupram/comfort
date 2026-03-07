use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Landmark {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Landmark {
    pub fn distance(&self, other: &Self) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackingStatus {
    Tracking,
    Lost,
    LowConfidence,
    Calibrating,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisionStatus {
    LoadingModel,
    LoadingCamera,
    Ready,
    Running,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlIntent {
    MoveDelta { dx: f32, dy: f32 },
    LeftClick,
    RightClick,
    Scroll { dy: f32 },
    ControlOn,
    ControlOff,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthMetrics {
    pub ts_ms: u64,
    pub fps_in: f32,
    pub fps_out: f32,
    pub latency_p50_ms: f32,
    pub latency_p95_ms: f32,
    pub dropped_frames: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackingFrame {
    pub ts_ms: u64,
    pub frame_id: u64,
    pub confidence: f32,
    pub landmarks: Vec<Landmark>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CameraPreview {
    pub ts_ms: u64,
    pub frame_id: u64,
    pub jpeg_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GestureHint {
    pub ts_ms: u64,
    pub frame_id: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeEvent {
    VisionStatus { status: VisionStatus },
    TrackingStatus { status: TrackingStatus },
    TrackingFrame { frame: TrackingFrame },
    CameraPreview { frame: CameraPreview },
    GestureHint { hint: GestureHint },
    ControlIntent { intent: ControlIntent },
    HealthMetrics { metrics: HealthMetrics },
}
