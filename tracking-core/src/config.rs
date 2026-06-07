use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisionBackend {
    Mock,
    #[default]
    PythonMediapipe,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationProfile {
    Comfort,
    #[default]
    Balanced,
    Responsive,
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
        const CONFIG_VERSION: u32 = 1;
        const MIN_LOST_TIMEOUT_MS: u64 = 500;
        const DEFAULT_LOST_TIMEOUT_MS: u64 = 700;
        const MAX_LOST_TIMEOUT_MS: u64 = 10_000;
        const MIN_CONF_GAP: f32 = 0.10;

        self.config_version = CONFIG_VERSION;
        self.python_executable = self.python_executable.trim().to_string();
        if self.python_executable.is_empty() {
            self.python_executable = Self::default().python_executable;
        }
        if let Some(path) = self.mediapipe_script_path.as_deref() {
            let trimmed = path.trim();
            self.mediapipe_script_path = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
        self.camera_index = self.camera_index.max(0);
        self.camera_width = self.camera_width.clamp(160, 3840);
        self.camera_height = self.camera_height.clamp(120, 2160);
        self.camera_fps = self.camera_fps.clamp(1, 120);

        self.confidence_lock = clamp_finite(self.confidence_lock, 0.35, 0.95, 0.65);
        self.confidence_unlock = clamp_finite(self.confidence_unlock, 0.20, 0.90, 0.45);
        if self.lost_timeout_ms < MIN_LOST_TIMEOUT_MS {
            self.lost_timeout_ms = DEFAULT_LOST_TIMEOUT_MS;
        }
        self.lost_timeout_ms = self.lost_timeout_ms.min(MAX_LOST_TIMEOUT_MS);

        if self.confidence_unlock >= self.confidence_lock {
            self.confidence_unlock = (self.confidence_lock - MIN_CONF_GAP).max(0.20);
        }

        self.min_cutoff = clamp_finite(self.min_cutoff, 0.01, 10.0, 1.0);
        self.beta = clamp_finite(self.beta, 0.0, 2.0, 0.02);
        self.d_cutoff = clamp_finite(self.d_cutoff, 0.01, 10.0, 1.0);

        self.pointer_range = clamp_finite(self.pointer_range, 0.5, 3.0, 1.0);
        self.move_gain = clamp_finite(self.move_gain, 1.0, 15.0, 5.0);
        self.move_accel = clamp_finite(self.move_accel, 0.0, 30.0, 8.0);
        self.move_max_delta = clamp_finite(self.move_max_delta, 0.01, 0.25, 0.08);
        self.deadzone = clamp_finite(self.deadzone, 0.001, 0.08, 0.015);
        self.hold_to_control_ms = self.hold_to_control_ms.clamp(20, 300);
        self.clutch_enter_ms = self.clutch_enter_ms.clamp(20, 300);
        self.pinch_threshold = clamp_finite(self.pinch_threshold, 0.015, 0.090, 0.035);
        self.right_pinch_threshold = clamp_finite(self.right_pinch_threshold, 0.015, 0.100, 0.040);
        self.click_cooldown_ms = self.click_cooldown_ms.clamp(60, 450);
        self.scroll_mode_threshold = clamp_finite(self.scroll_mode_threshold, 0.015, 0.120, 0.045);
        self.scroll_gain = clamp_finite(self.scroll_gain, 1.0, 80.0, 28.0);
        self.scroll_deadzone = clamp_finite(self.scroll_deadzone, 0.0, 5.0, 0.6);
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
        write_file_atomically(path, json.as_bytes())
            .with_context(|| format!("write settings {:?}", path))?;
        Ok(())
    }
}

fn write_file_atomically(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    let tmp_path = temporary_save_path(path)?;
    let write_result = (|| -> anyhow::Result<()> {
        {
            let mut tmp = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)
                .with_context(|| format!("create temp file {:?}", tmp_path))?;
            tmp.write_all(data)
                .with_context(|| format!("write temp file {:?}", tmp_path))?;
            tmp.sync_all()
                .with_context(|| format!("sync temp file {:?}", tmp_path))?;
        }
        replace_file(&tmp_path, path)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    write_result
}

fn temporary_save_path(path: &Path) -> anyhow::Result<std::path::PathBuf> {
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("settings path has no file name"))?
        .to_string_lossy();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Ok(path.with_file_name(format!("{file_name}.{}.{}.tmp", std::process::id(), nonce)))
}

#[cfg(not(windows))]
fn replace_file(tmp_path: &Path, path: &Path) -> anyhow::Result<()> {
    fs::rename(tmp_path, path).with_context(|| format!("rename {:?} to {:?}", tmp_path, path))
}

#[cfg(windows)]
fn replace_file(tmp_path: &Path, path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("remove existing file {:?}", path))?;
    }
    fs::rename(tmp_path, path).with_context(|| format!("rename {:?} to {:?}", tmp_path, path))
}

fn clamp_finite(value: f32, min: f32, max: f32, default: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        default
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
        assert_eq!(fs::read_dir(dir.path()).expect("read tempdir").count(), 1);
    }

    #[test]
    fn failed_save_removes_temp_file_and_preserves_existing_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        fs::create_dir(&path).expect("create existing directory");

        let err = AppSettings::default()
            .save(&path)
            .expect_err("save should fail");

        assert!(err.to_string().contains("write settings"));
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(dir.path()).expect("read tempdir").count(), 1);
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

    #[test]
    fn normalizes_invalid_runtime_bounds() {
        let out = AppSettings {
            config_version: 0,
            python_executable: "  ".to_string(),
            mediapipe_script_path: Some(" ".to_string()),
            camera_index: -10,
            camera_width: 0,
            camera_height: 8_000,
            camera_fps: 0,
            confidence_lock: f32::NAN,
            confidence_unlock: 0.99,
            lost_timeout_ms: 90_000,
            min_cutoff: f32::INFINITY,
            beta: -1.0,
            d_cutoff: 0.0,
            pointer_range: f32::NAN,
            move_gain: 0.0,
            move_accel: 99.0,
            move_max_delta: 0.0,
            deadzone: 1.0,
            hold_to_control_ms: 1,
            clutch_enter_ms: 1_000,
            pinch_threshold: f32::NAN,
            right_pinch_threshold: 99.0,
            click_cooldown_ms: 1,
            scroll_mode_threshold: 0.0,
            scroll_gain: f32::NAN,
            scroll_deadzone: 99.0,
            ..AppSettings::default()
        }
        .normalized();

        assert_eq!(out.config_version, 1);
        assert_eq!(out.python_executable, "python");
        assert_eq!(out.mediapipe_script_path, None);
        assert_eq!(out.camera_index, 0);
        assert_eq!(out.camera_width, 160);
        assert_eq!(out.camera_height, 2160);
        assert_eq!(out.camera_fps, 1);
        assert_eq!(out.confidence_lock, 0.65);
        assert!(out.confidence_unlock < out.confidence_lock);
        assert_eq!(out.lost_timeout_ms, 10_000);
        assert_eq!(out.min_cutoff, 1.0);
        assert_eq!(out.beta, 0.0);
        assert_eq!(out.d_cutoff, 0.01);
        assert_eq!(out.pointer_range, 1.0);
        assert_eq!(out.move_gain, 1.0);
        assert_eq!(out.move_accel, 30.0);
        assert_eq!(out.move_max_delta, 0.01);
        assert_eq!(out.deadzone, 0.08);
        assert_eq!(out.hold_to_control_ms, 20);
        assert_eq!(out.clutch_enter_ms, 300);
        assert_eq!(out.pinch_threshold, 0.035);
        assert_eq!(out.right_pinch_threshold, 0.100);
        assert_eq!(out.click_cooldown_ms, 60);
        assert_eq!(out.scroll_mode_threshold, 0.015);
        assert_eq!(out.scroll_gain, 28.0);
        assert_eq!(out.scroll_deadzone, 5.0);
    }
}
