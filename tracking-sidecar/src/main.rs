mod config;
mod gestures;
mod health;
mod ipc;
mod pipeline;
mod smoothing;
mod trace;
mod types;

use anyhow::Context;
use clap::Parser;
use config::{Cli, Config};
use gestures::GestureEngine;
use health::HealthTracker;
use pipeline::{MockTracker, Tracker};
use smoothing::OneEuroSmoother;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use trace::TraceWriter;
use types::{IpcMessage, TrackingEvent, TrackingState};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = Config::from_cli(&cli).context("load config")?;
    let mut tracker = MockTracker::new();
    let mut smooth = OneEuroSmoother::new(21, cfg.min_cutoff, cfg.beta, cfg.d_cutoff);
    let mut engine = GestureEngine::new(cfg.clone());
    let mut health = HealthTracker::new();
    let mut trace = match &cli.trace_file {
        Some(path) => Some(TraceWriter::new(path)?),
        None => None,
    };

    let frame_interval = Duration::from_millis((1000.0 / cli.fps.max(1) as f32) as u64);
    let mut last_loop = Instant::now();
    let mut frame_id = 0u64;
    let mut last_seen_ok_ms = now_ms();

    loop {
        let loop_start = Instant::now();
        let ts_ms = now_ms();
        let packet = tracker.next(&cli, frame_id, ts_ms);
        let mut had_output = false;
        let dropped = packet.landmarks.is_none();

        if packet.confidence >= cfg.confidence_lock {
            last_seen_ok_ms = ts_ms;
        }

        let is_lost = ts_ms.saturating_sub(last_seen_ok_ms) > cfg.lost_timeout_ms
            || packet.confidence < cfg.confidence_unlock
            || packet.landmarks.is_none();

        if is_lost {
            let t_ev = TrackingEvent {
                ts_ms: packet.ts_ms,
                frame_id: packet.frame_id,
                state: TrackingState::Lost,
                confidence: packet.confidence,
                landmarks: Vec::new(),
            };
            emit_and_trace(&IpcMessage::TrackingEvent(t_ev), &mut trace)?;
            had_output = true;

            for intent in engine.reset_on_lost(packet.ts_ms) {
                emit_and_trace(&IpcMessage::ControlIntent(intent), &mut trace)?;
                had_output = true;
            }
        } else if let Some(landmarks) = packet.landmarks {
            let dt_s = last_loop.elapsed().as_secs_f32();
            let filtered = smooth.filter(&landmarks, dt_s);

            let state = if is_open_palm_like(&filtered) {
                TrackingState::PausedCandidate
            } else {
                TrackingState::Tracking
            };
            let t_ev = TrackingEvent {
                ts_ms: packet.ts_ms,
                frame_id: packet.frame_id,
                state,
                confidence: packet.confidence,
                landmarks: filtered.clone(),
            };
            emit_and_trace(&IpcMessage::TrackingEvent(t_ev), &mut trace)?;
            had_output = true;

            for intent in engine.process(packet.ts_ms, &filtered) {
                emit_and_trace(&IpcMessage::ControlIntent(intent), &mut trace)?;
                had_output = true;
            }
        }

        let latency_ms = loop_start.elapsed().as_secs_f32() * 1000.0;
        health.on_frame(latency_ms, had_output, dropped);
        if let Some(h) = health.maybe_emit(ts_ms) {
            emit_and_trace(&IpcMessage::HealthEvent(h), &mut trace)?;
        }

        frame_id += 1;
        if cli.max_frames > 0 && frame_id >= cli.max_frames {
            break;
        }

        let elapsed = loop_start.elapsed();
        if elapsed < frame_interval {
            thread::sleep(frame_interval - elapsed);
        }
        last_loop = loop_start;
    }

    if let Some(t) = &mut trace {
        t.flush()?;
    }
    Ok(())
}

fn emit_and_trace(msg: &IpcMessage, trace: &mut Option<TraceWriter>) -> anyhow::Result<()> {
    ipc::emit(msg)?;
    if let Some(w) = trace {
        w.write(msg)?;
    }
    Ok(())
}

fn is_open_palm_like(landmarks: &[types::Landmark]) -> bool {
    if landmarks.len() < 21 {
        return false;
    }
    landmarks[8].y < landmarks[6].y
        && landmarks[12].y < landmarks[10].y
        && landmarks[16].y < landmarks[14].y
        && landmarks[20].y < landmarks[18].y
}
