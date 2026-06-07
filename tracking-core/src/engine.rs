use crate::config::AppSettings;
use crate::gestures::GestureEngine;
use crate::smoothing::OneEuroSmoother;
use crate::telemetry::HealthTracker;
use crate::tracker::{create_tracker, VisionTracker};
use crate::types::{CameraPreview, GestureHint, RuntimeEvent, TrackingFrame, TrackingStatus};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarmupState {
    NoFrame,
    FrameAvailable,
    HandVisible,
}

pub struct RuntimeEngine {
    settings: AppSettings,
    tracker: Option<Box<dyn VisionTracker>>,
    smoother: OneEuroSmoother,
    smoothing_scratch: Vec<crate::types::Landmark>,
    gestures: GestureEngine,
    intent_scratch: Vec<crate::types::ControlIntent>,
    telemetry: HealthTracker,
    last_seen_ok_ms: u64,
    last_seen_landmark_ms: u64,
    lost_since_ms: Option<u64>,
    last_status: Option<TrackingStatus>,
    last_tick: Instant,
}

impl RuntimeEngine {
    pub fn new(settings: AppSettings) -> Self {
        Self {
            smoother: OneEuroSmoother::new(
                21,
                settings.min_cutoff,
                settings.beta,
                settings.d_cutoff,
            ),
            smoothing_scratch: Vec::with_capacity(21),
            gestures: GestureEngine::new(&settings),
            intent_scratch: Vec::with_capacity(2),
            settings,
            tracker: None,
            telemetry: HealthTracker::new(),
            last_seen_ok_ms: 0,
            last_seen_landmark_ms: 0,
            lost_since_ms: None,
            last_status: None,
            last_tick: Instant::now(),
        }
    }

    pub fn update_settings(&mut self, settings: AppSettings) -> bool {
        let tracker_changed = tracker_settings_changed(&self.settings, &settings);
        if tracker_changed || smoothing_settings_changed(&self.settings, &settings) {
            self.smoother =
                OneEuroSmoother::new(21, settings.min_cutoff, settings.beta, settings.d_cutoff);
            self.smoothing_scratch.clear();
        }
        self.gestures.update_settings(&settings);
        if tracker_changed {
            self.release_tracker();
        }
        self.settings = settings;
        tracker_changed
    }

    pub fn stop(&mut self, ts_ms: u64) -> Vec<RuntimeEvent> {
        self.release_tracker();
        let mut out = Vec::with_capacity(1);
        self.gestures
            .reset_on_lost_into(ts_ms, &mut self.intent_scratch);
        self.drain_control_intents(&mut out);
        out
    }

    pub fn tick(&mut self, ts_ms: u64, paused: bool) -> Vec<RuntimeEvent> {
        let mut out = Vec::new();
        let loop_start = Instant::now();

        if paused {
            let packet = self.next_packet(ts_ms);
            if let Some(preview) = packet.preview_jpeg_base64 {
                out.push(RuntimeEvent::CameraPreview {
                    frame: CameraPreview {
                        ts_ms: packet.ts_ms,
                        frame_id: packet.frame_id,
                        jpeg_base64: preview,
                    },
                });
            }
            if let Some(landmarks) = packet.landmarks {
                if landmarks.len() == 21 {
                    let dt_s = self.last_tick.elapsed().as_secs_f32().max(1.0 / 120.0);
                    self.smoother
                        .filter_into(&landmarks, dt_s, &mut self.smoothing_scratch);
                    let hint = self.gestures.hint(ts_ms, &self.smoothing_scratch);
                    out.push(RuntimeEvent::GestureHint {
                        hint: GestureHint {
                            ts_ms: packet.ts_ms,
                            frame_id: packet.frame_id,
                            label: hint.label.to_string(),
                            control_mode: hint.control_mode,
                            move_active: hint.move_active,
                            two_finger_pose: hint.two_finger_pose,
                            open_palm_pose: hint.open_palm_pose,
                            closed_palm_pose: hint.closed_palm_pose,
                            hand_down_pose: hint.hand_down_pose,
                            scroll_mode: hint.scroll_mode,
                            pinch_index: hint.pinch_index,
                            pinch_middle: hint.pinch_middle,
                            pinch_index_threshold: hint.pinch_index_threshold,
                            pinch_middle_threshold: hint.pinch_middle_threshold,
                            hold_progress: hint.hold_progress,
                            thumb_index_distance: hint.thumb_index_distance,
                            thumb_middle_distance: hint.thumb_middle_distance,
                            index_middle_distance: hint.index_middle_distance,
                            index_extended: hint.index_extended,
                            middle_extended: hint.middle_extended,
                            ring_extended: hint.ring_extended,
                            pinky_extended: hint.pinky_extended,
                        },
                    });
                }
            }
            let latency_ms = loop_start.elapsed().as_secs_f32() * 1000.0;
            self.telemetry
                .on_frame(latency_ms, !out.is_empty(), packet.dropped);
            if let Some(h) = self.telemetry.maybe_emit(ts_ms) {
                out.push(RuntimeEvent::HealthMetrics { metrics: h });
            }
            self.last_tick = loop_start;
            return out;
        }

        let packet = self.next_packet(ts_ms);
        let has_landmarks = packet.landmarks.is_some();
        if let Some(preview) = packet.preview_jpeg_base64 {
            out.push(RuntimeEvent::CameraPreview {
                frame: CameraPreview {
                    ts_ms: packet.ts_ms,
                    frame_id: packet.frame_id,
                    jpeg_base64: preview,
                },
            });
        }
        if has_landmarks {
            self.last_seen_landmark_ms = ts_ms;
        }
        if packet.confidence >= self.settings.confidence_lock && has_landmarks {
            self.last_seen_ok_ms = ts_ms;
        }

        let status = if !has_landmarks
            && ts_ms.saturating_sub(self.last_seen_landmark_ms) > self.settings.lost_timeout_ms
        {
            TrackingStatus::Lost
        } else if packet.confidence < self.settings.confidence_unlock {
            TrackingStatus::LowConfidence
        } else {
            TrackingStatus::Tracking
        };
        if status == TrackingStatus::Lost {
            if self.lost_since_ms.is_none() {
                self.lost_since_ms = Some(ts_ms);
            }
        } else {
            self.lost_since_ms = None;
        }
        let lost_reset_grace_ms = self.settings.lost_timeout_ms.max(900);
        let should_reset_on_lost = status == TrackingStatus::Lost
            && self
                .lost_since_ms
                .map(|since| ts_ms.saturating_sub(since) >= lost_reset_grace_ms)
                .unwrap_or(false);
        self.emit_status_if_changed(&mut out, status);

        let mut emitted_landmark_hint = false;
        if let Some(landmarks) = packet.landmarks {
            if landmarks.len() == 21 {
                emitted_landmark_hint = true;
                let dt_s = self.last_tick.elapsed().as_secs_f32().max(1.0 / 120.0);
                let filtered = self.smoother.filter(&landmarks, dt_s);
                if status != TrackingStatus::Lost {
                    self.gestures
                        .process_into(ts_ms, &filtered, &mut self.intent_scratch);
                    self.drain_control_intents(&mut out);
                } else if should_reset_on_lost {
                    self.gestures
                        .reset_on_lost_into(ts_ms, &mut self.intent_scratch);
                    self.drain_control_intents(&mut out);
                }
                let hint = self.gestures.hint(ts_ms, &filtered);
                out.push(RuntimeEvent::GestureHint {
                    hint: GestureHint {
                        ts_ms: packet.ts_ms,
                        frame_id: packet.frame_id,
                        label: hint.label.to_string(),
                        control_mode: hint.control_mode,
                        move_active: hint.move_active,
                        two_finger_pose: hint.two_finger_pose,
                        open_palm_pose: hint.open_palm_pose,
                        closed_palm_pose: hint.closed_palm_pose,
                        hand_down_pose: hint.hand_down_pose,
                        scroll_mode: hint.scroll_mode,
                        pinch_index: hint.pinch_index,
                        pinch_middle: hint.pinch_middle,
                        pinch_index_threshold: hint.pinch_index_threshold,
                        pinch_middle_threshold: hint.pinch_middle_threshold,
                        hold_progress: hint.hold_progress,
                        thumb_index_distance: hint.thumb_index_distance,
                        thumb_middle_distance: hint.thumb_middle_distance,
                        index_middle_distance: hint.index_middle_distance,
                        index_extended: hint.index_extended,
                        middle_extended: hint.middle_extended,
                        ring_extended: hint.ring_extended,
                        pinky_extended: hint.pinky_extended,
                    },
                });
                out.push(RuntimeEvent::TrackingFrame {
                    frame: TrackingFrame {
                        ts_ms: packet.ts_ms,
                        frame_id: packet.frame_id,
                        confidence: packet.confidence,
                        landmarks: filtered,
                    },
                });
            }
        }

        if status == TrackingStatus::Lost && !emitted_landmark_hint && should_reset_on_lost {
            self.gestures
                .reset_on_lost_into(ts_ms, &mut self.intent_scratch);
            self.drain_control_intents(&mut out);
        }

        let latency_ms = loop_start.elapsed().as_secs_f32() * 1000.0;
        self.telemetry
            .on_frame(latency_ms, !out.is_empty(), packet.dropped);
        if let Some(h) = self.telemetry.maybe_emit(ts_ms) {
            out.push(RuntimeEvent::HealthMetrics { metrics: h });
        }
        self.last_tick = loop_start;
        out
    }

    pub fn warmup_tick(&mut self, ts_ms: u64) -> WarmupState {
        let packet = self.next_packet(ts_ms);
        if packet.dropped {
            return WarmupState::NoFrame;
        }
        if let Some(landmarks) = packet.landmarks {
            if landmarks.len() == 21 {
                let dt_s = self.last_tick.elapsed().as_secs_f32().max(1.0 / 120.0);
                self.smoother
                    .filter_into(&landmarks, dt_s, &mut self.smoothing_scratch);
                self.last_tick = Instant::now();
                return WarmupState::HandVisible;
            }
        }
        WarmupState::FrameAvailable
    }

    fn emit_status_if_changed(&mut self, out: &mut Vec<RuntimeEvent>, status: TrackingStatus) {
        if self.last_status != Some(status) {
            out.push(RuntimeEvent::TrackingStatus { status });
            self.last_status = Some(status);
        }
    }

    fn next_packet(&mut self, ts_ms: u64) -> crate::tracker::TrackerPacket {
        if self.tracker.is_none() {
            self.tracker = Some(create_tracker(&self.settings));
            self.last_tick = Instant::now();
        }
        self.tracker
            .as_mut()
            .expect("tracker initialized before use")
            .next(ts_ms)
    }

    fn release_tracker(&mut self) {
        self.tracker = None;
        self.intent_scratch.clear();
        self.smoothing_scratch.clear();
        self.last_seen_ok_ms = 0;
        self.last_seen_landmark_ms = 0;
        self.lost_since_ms = None;
        self.last_status = None;
        self.last_tick = Instant::now();
    }

    fn drain_control_intents(&mut self, out: &mut Vec<RuntimeEvent>) {
        out.extend(
            self.intent_scratch
                .drain(..)
                .map(|intent| RuntimeEvent::ControlIntent { intent }),
        );
    }
}

fn smoothing_settings_changed(current: &AppSettings, next: &AppSettings) -> bool {
    current.min_cutoff != next.min_cutoff
        || current.beta != next.beta
        || current.d_cutoff != next.d_cutoff
}

fn tracker_settings_changed(current: &AppSettings, next: &AppSettings) -> bool {
    current.vision_backend != next.vision_backend
        || current.python_executable != next.python_executable
        || current.mediapipe_script_path != next.mediapipe_script_path
        || current.camera_index != next.camera_index
        || current.camera_width != next.camera_width
        || current.camera_height != next.camera_height
        || current.camera_fps != next.camera_fps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_tracking_status() {
        let settings = AppSettings {
            vision_backend: crate::config::VisionBackend::Mock,
            ..AppSettings::default()
        };
        let mut engine = RuntimeEngine::new(settings);
        let events = engine.tick(10, false);
        assert!(events.iter().any(|e| matches!(
            e,
            RuntimeEvent::TrackingStatus {
                status: TrackingStatus::Tracking
            }
        )));
    }

    #[test]
    fn tracker_is_lazy_and_released_on_stop() {
        let settings = AppSettings {
            vision_backend: crate::config::VisionBackend::Mock,
            ..AppSettings::default()
        };
        let mut engine = RuntimeEngine::new(settings);

        assert!(engine.tracker.is_none());
        let _ = engine.tick(10, false);
        assert!(engine.tracker.is_some());
        let _ = engine.stop(20);
        assert!(engine.tracker.is_none());
    }

    #[test]
    fn warmup_reaches_frame_available() {
        let settings = AppSettings {
            vision_backend: crate::config::VisionBackend::Mock,
            ..AppSettings::default()
        };
        let mut engine = RuntimeEngine::new(settings);
        let state = engine.warmup_tick(10);
        assert!(matches!(
            state,
            WarmupState::FrameAvailable | WarmupState::HandVisible
        ));
    }

    #[test]
    fn tracker_restart_scope_is_limited_to_vision_settings() {
        let current = AppSettings::default();
        let gesture_only = AppSettings {
            move_gain: current.move_gain + 1.0,
            ..current.clone()
        };
        assert!(!tracker_settings_changed(&current, &gesture_only));

        let camera_change = AppSettings {
            camera_index: current.camera_index + 1,
            ..current
        };
        assert!(tracker_settings_changed(
            &AppSettings::default(),
            &camera_change
        ));
    }

    #[test]
    fn update_settings_reports_whether_tracker_changed() {
        let settings = AppSettings {
            vision_backend: crate::config::VisionBackend::Mock,
            ..AppSettings::default()
        };
        let mut engine = RuntimeEngine::new(settings.clone());

        let gesture_only = AppSettings {
            move_gain: settings.move_gain + 1.0,
            ..settings.clone()
        };
        assert!(!engine.update_settings(gesture_only));

        let vision_change = AppSettings {
            camera_width: settings.camera_width + 16,
            ..settings
        };
        assert!(engine.update_settings(vision_change));
    }

    #[test]
    fn smoother_restart_scope_is_limited_to_smoothing_settings() {
        let current = AppSettings::default();
        let gesture_only = AppSettings {
            move_gain: current.move_gain + 1.0,
            ..current.clone()
        };
        assert!(!smoothing_settings_changed(&current, &gesture_only));

        let smoothing_change = AppSettings {
            beta: current.beta + 0.01,
            ..current
        };
        assert!(smoothing_settings_changed(
            &AppSettings::default(),
            &smoothing_change
        ));
    }
}
