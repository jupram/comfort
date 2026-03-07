use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(author, version, about = "Phase 1 hand-tracking sidecar")]
pub struct Cli {
    #[arg(long, default_value_t = 0)]
    pub camera_index: i32,
    #[arg(long, default_value_t = 1920)]
    pub width: u32,
    #[arg(long, default_value_t = 1080)]
    pub height: u32,
    #[arg(long, default_value_t = 30)]
    pub fps: u32,
    #[arg(long)]
    pub trace_file: Option<PathBuf>,
    #[arg(long)]
    pub profile: Option<PathBuf>,
    #[arg(long, default_value_t = 0)]
    pub max_frames: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub move_pose_hold_ms: u64,
    pub pause_hold_ms: u64,
    pub double_click_window_ms: u64,
    pub pinch_threshold: f32,
    pub move_gain: f32,
    pub deadzone: f32,
    pub confidence_lock: f32,
    pub confidence_unlock: f32,
    pub lost_timeout_ms: u64,
    pub min_cutoff: f32,
    pub beta: f32,
    pub d_cutoff: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            move_pose_hold_ms: 70,
            pause_hold_ms: 180,
            double_click_window_ms: 280,
            pinch_threshold: 0.035,
            move_gain: 4.5,
            deadzone: 0.015,
            confidence_lock: 0.65,
            confidence_unlock: 0.45,
            lost_timeout_ms: 250,
            min_cutoff: 1.0,
            beta: 0.02,
            d_cutoff: 1.0,
        }
    }
}

impl Config {
    pub fn from_cli(cli: &Cli) -> anyhow::Result<Self> {
        if let Some(profile) = &cli.profile {
            let raw = fs::read_to_string(profile)?;
            let cfg = serde_json::from_str::<Config>(&raw)?;
            Ok(cfg)
        } else {
            Ok(Self::default())
        }
    }
}
