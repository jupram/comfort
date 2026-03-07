use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisionBackend {
    Mock,
    PythonMediapipe,
}

impl Default for VisionBackend {
    fn default() -> Self {
        Self::PythonMediapipe
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationProfile {
    Comfort,
    Balanced,
    Responsive,
}

impl Default for CalibrationProfile {
    fn default() -> Self {
        Self::Balanced
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppSettings {
    pub config_version: u32,
    pub calibration_profile: CalibrationProfile,
    pub calibrated_at_ms: Option<u64>,
    pub vision_backend: VisionBackend,
    pub python_executable: String,
    pub mediapipe_script_path: Option<String>,
    pub camera_index: i32,
    pub camera_width: u32,
    pub camera_height: u32,
    pub camera_fps: u32,
    pub confidence_lock: f32,
    pub confidence_unlock: f32,
    pub lost_timeout_ms: u64,
    pub min_cutoff: f32,
    pub beta: f32,
    pub d_cutoff: f32,
    pub input_injection_enabled: bool,
    pub safe_mode: bool,
    pub allow_safe_mode_movement: bool,
    pub pointer_range: f32,
    pub move_gain: f32,
    pub move_accel: f32,
    pub move_max_delta: f32,
    pub deadzone: f32,
    pub hold_to_control_ms: u64,
    pub clutch_enter_ms: u64,
    pub pinch_threshold: f32,
    pub right_pinch_threshold: f32,
    pub click_cooldown_ms: u64,
    pub scroll_mode_threshold: f32,
    pub scroll_gain: f32,
    pub scroll_deadzone: f32,
    pub diagnostics_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            config_version: 1,
            calibration_profile: CalibrationProfile::Balanced,
            calibrated_at_ms: None,
            vision_backend: VisionBackend::PythonMediapipe,
            python_executable: "python".to_string(),
            mediapipe_script_path: Some("vision/mediapipe_tracker.py".to_string()),
            camera_index: 0,
            camera_width: 1280,
            camera_height: 720,
            camera_fps: 30,
            confidence_lock: 0.65,
            confidence_unlock: 0.45,
            lost_timeout_ms: 700,
            min_cutoff: 1.0,
            beta: 0.02,
            d_cutoff: 1.0,
            input_injection_enabled: false,
            safe_mode: true,
            allow_safe_mode_movement: false,
            pointer_range: 1.0,
            move_gain: 5.0,
            move_accel: 8.0,
            move_max_delta: 0.08,
            deadzone: 0.015,
            hold_to_control_ms: 70,
            clutch_enter_ms: 70,
            pinch_threshold: 0.035,
            right_pinch_threshold: 0.040,
            click_cooldown_ms: 160,
            scroll_mode_threshold: 0.045,
            scroll_gain: 28.0,
            scroll_deadzone: 0.6,
            diagnostics_enabled: true,
        }
    }
}

impl AppSettings {
    pub fn normalize_in_place(&mut self) {
        const MIN_LOST_TIMEOUT_MS: u64 = 500;
        const DEFAULT_LOST_TIMEOUT_MS: u64 = 700;
        const MIN_CONF_GAP: f32 = 0.10;

        if self.lost_timeout_ms < MIN_LOST_TIMEOUT_MS {
            self.lost_timeout_ms = DEFAULT_LOST_TIMEOUT_MS;
        }

        if self.confidence_unlock >= self.confidence_lock {
            self.confidence_unlock = (self.confidence_lock - MIN_CONF_GAP).max(0.20);
        }

        self.move_accel = self.move_accel.clamp(0.0, 30.0);
        self.move_max_delta = self.move_max_delta.clamp(0.01, 0.25);
        self.clutch_enter_ms = self.clutch_enter_ms.clamp(20, 300);
        self.pointer_range = self.pointer_range.clamp(0.5, 3.0);
    }

    pub fn normalized(mut self) -> Self {
        self.normalize_in_place();
        self
    }

    pub fn apply_calibration_profile(&mut self, profile: CalibrationProfile) {
        self.calibration_profile = profile;
        match profile {
            CalibrationProfile::Comfort => {
                self.pointer_range = 0.95;
                self.move_gain = 4.6;
                self.move_accel = 6.4;
                self.move_max_delta = 0.075;
                self.deadzone = 0.018;
                self.hold_to_control_ms = 85;
                self.clutch_enter_ms = 80;
                self.pinch_threshold = 0.038;
                self.right_pinch_threshold = 0.043;
                self.click_cooldown_ms = 190;
                self.confidence_lock = 0.68;
                self.confidence_unlock = 0.48;
            }
            CalibrationProfile::Balanced => {
                self.pointer_range = 1.0;
                self.move_gain = 5.5;
                self.move_accel = 8.0;
                self.move_max_delta = 0.085;
                self.deadzone = 0.012;
                self.hold_to_control_ms = 70;
                self.clutch_enter_ms = 70;
                self.pinch_threshold = 0.035;
                self.right_pinch_threshold = 0.040;
                self.click_cooldown_ms = 170;
                self.confidence_lock = 0.65;
                self.confidence_unlock = 0.45;
            }
            CalibrationProfile::Responsive => {
                self.pointer_range = 1.15;
                self.move_gain = 6.8;
                self.move_accel = 10.5;
                self.move_max_delta = 0.095;
                self.deadzone = 0.008;
                self.hold_to_control_ms = 55;
                self.clutch_enter_ms = 60;
                self.pinch_threshold = 0.032;
                self.right_pinch_threshold = 0.036;
                self.click_cooldown_ms = 140;
                self.confidence_lock = 0.60;
                self.confidence_unlock = 0.40;
            }
        }
    }

    pub fn load_or_default(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path).with_context(|| format!("read settings {:?}", path))?;
        let cfg = serde_json::from_str::<Self>(&raw)
            .context("parse settings JSON")?
            .normalized();
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create dir {:?}", parent))?;
        }
        let json = serde_json::to_string_pretty(&self.clone().normalized())
            .context("serialize settings JSON")?;
        fs::write(path, json).with_context(|| format!("write settings {:?}", path))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");

        let input = AppSettings {
            input_injection_enabled: true,
            allow_safe_mode_movement: true,
            ..AppSettings::default()
        };
        input.save(&path).expect("save");
        let out = AppSettings::load_or_default(&path).expect("load");

        assert_eq!(input, out);
    }

    #[test]
    fn normalizes_legacy_lost_timeout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let legacy = r#"{
  "config_version": 1,
  "calibration_profile": "balanced",
  "calibrated_at_ms": null,
  "vision_backend": "python_mediapipe",
  "python_executable": "python",
  "mediapipe_script_path": "vision/mediapipe_tracker.py",
  "camera_index": 0,
  "camera_width": 1280,
  "camera_height": 720,
  "camera_fps": 30,
  "confidence_lock": 0.65,
  "confidence_unlock": 0.45,
  "lost_timeout_ms": 250,
  "min_cutoff": 1.0,
  "beta": 0.02,
  "d_cutoff": 1.0,
  "input_injection_enabled": false,
  "safe_mode": true,
  "allow_safe_mode_movement": false,
  "move_gain": 5.0,
  "deadzone": 0.015,
  "hold_to_control_ms": 70,
  "pinch_threshold": 0.035,
  "right_pinch_threshold": 0.04,
  "click_cooldown_ms": 160,
  "scroll_mode_threshold": 0.045,
  "scroll_gain": 28.0,
  "scroll_deadzone": 0.6,
  "diagnostics_enabled": true
}"#;
        fs::write(&path, legacy).expect("write");
        let out = AppSettings::load_or_default(&path).expect("load");
        assert_eq!(out.lost_timeout_ms, 700);
    }
}
