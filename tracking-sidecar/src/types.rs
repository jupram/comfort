use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TrackingState {
    Tracking,
    Lost,
    PausedCandidate,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize)]
pub struct TrackingEvent {
    pub ts_ms: u64,
    pub frame_id: u64,
    pub state: TrackingState,
    pub confidence: f32,
    pub landmarks: Vec<Landmark>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ControlType {
    MoveActiveOn,
    MoveActiveOff,
    MoveDelta,
    Click,
    DoubleClick,
    PauseRequest,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Delta {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ControlIntent {
    pub ts_ms: u64,
    #[serde(rename = "type")]
    pub intent_type: ControlType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<Delta>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthEvent {
    pub ts_ms: u64,
    pub fps_in: f32,
    pub fps_out: f32,
    pub latency_ms_p50: f32,
    pub latency_ms_p95: f32,
    pub dropped_frames: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IpcMessage {
    TrackingEvent(TrackingEvent),
    ControlIntent(ControlIntent),
    HealthEvent(HealthEvent),
}
