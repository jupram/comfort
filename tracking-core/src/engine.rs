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
    tracker: Box<dyn VisionTracker>,
    smoother: OneEuroSmoother,
    gestures: GestureEngine,
    telemetry: HealthTracker,
    last_seen_ok_ms: u64,
    last_seen_landmark_ms: u64,
    lost_since_ms: Option<u64>,
    last_status: Option<TrackingStatus>,
    last_tick: Instant,
}

impl RuntimeEngine {
    pub fn new(settings: AppSettings) -> Self {
        let tracker = create_tracker(&settings);
        Self {
            smoother: OneEuroSmoother::new(
                21,
                settings.min_cutoff,
                settings.beta,
                settings.d_cutoff,
            ),
            gestures: GestureEngine::new(settings.clone()),
            settings,
            tracker,
            telemetry: HealthTracker::new(),
            last_seen_ok_ms: 0,
            last_seen_landmark_ms: 0,
            lost_since_ms: None,
            last_status: None,
            last_tick: Instant::now(),
        }
    }

    pub fn update_settings(&mut self, settings: AppSettings) {
        self.smoother =
            OneEuroSmoother::new(21, settings.min_cutoff, settings.beta, settings.d_cutoff);
        self.gestures.update_settings(settings.clone());
        self.tracker = create_tracker(&settings);
        self.settings = settings;
    }

    pub fn tick(&mut self, ts_ms: u64, paused: bool) -> Vec<RuntimeEvent> {
        let mut out = Vec::new();
        let loop_start = Instant::now();

        if paused {
            let packet = self.tracker.next(ts_ms);
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
                    let filtered = self.smoother.filter(&landmarks, dt_s);
                    let hint = self.gestures.hint(ts_ms, &filtered);
                    out.push(RuntimeEvent::GestureHint {
                        hint: GestureHint {
                            ts_ms: packet.ts_ms,
                            frame_id: packet.frame_id,
                            label: hint.label,
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
            if let Some(h) = self.telemetry.maybe_emit(ts_ms) {
                out.push(RuntimeEvent::HealthMetrics { metrics: h });
            }
            self.telemetry.on_frame(0.1, false, packet.dropped);
            self.last_tick = loop_start;
            return out;
        }

        let packet = self.tracker.next(ts_ms);
        let has_landmarks = packet.landmarks.is_some();
        if let Some(preview) = packet.preview_jpeg_base64.clone() {
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
                    for intent in self.gestures.process(ts_ms, &filtered) {
                        out.push(RuntimeEvent::ControlIntent { intent });
                    }
                } else if should_reset_on_lost {
                    for intent in self.gestures.reset_on_lost(ts_ms) {
                        out.push(RuntimeEvent::ControlIntent { intent });
                    }
                }
                let hint = self.gestures.hint(ts_ms, &filtered);
                out.push(RuntimeEvent::GestureHint {
                    hint: GestureHint {
                        ts_ms: packet.ts_ms,
                        frame_id: packet.frame_id,
                        label: hint.label,
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
            for intent in self.gestures.reset_on_lost(ts_ms) {
                out.push(RuntimeEvent::ControlIntent { intent });
            }
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
        let packet = self.tracker.next(ts_ms);
        if packet.dropped {
            return WarmupState::NoFrame;
        }
        if let Some(landmarks) = packet.landmarks {
            if landmarks.len() == 21 {
                let dt_s = self.last_tick.elapsed().as_secs_f32().max(1.0 / 120.0);
                let _ = self.smoother.filter(&landmarks, dt_s);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_tracking_status() {
        let mut settings = AppSettings::default();
        settings.vision_backend = crate::config::VisionBackend::Mock;
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
    fn warmup_reaches_frame_available() {
        let mut settings = AppSettings::default();
        settings.vision_backend = crate::config::VisionBackend::Mock;
        let mut engine = RuntimeEngine::new(settings);
        let state = engine.warmup_tick(10);
        assert!(matches!(
            state,
            WarmupState::FrameAvailable | WarmupState::HandVisible
        ));
    }
}
