use crate::types::HealthMetrics;
use std::collections::VecDeque;

pub struct HealthTracker {
    samples_ms: VecDeque<f32>,
    in_count: u64,
    out_count: u64,
    dropped_frames: u64,
    last_emit_ms: u64,
}

impl HealthTracker {
    pub fn new() -> Self {
        Self {
            samples_ms: VecDeque::new(),
            in_count: 0,
            out_count: 0,
            dropped_frames: 0,
            last_emit_ms: 0,
        }
    }

    pub fn on_frame(&mut self, latency_ms: f32, emitted_output: bool, dropped: bool) {
        self.in_count += 1;
        if emitted_output {
            self.out_count += 1;
        }
        if dropped {
            self.dropped_frames += 1;
        }
        self.samples_ms.push_back(latency_ms);
        while self.samples_ms.len() > 240 {
            let _ = self.samples_ms.pop_front();
        }
    }

    pub fn maybe_emit(&mut self, ts_ms: u64) -> Option<HealthMetrics> {
        if self.last_emit_ms != 0 && ts_ms.saturating_sub(self.last_emit_ms) < 1_000 {
            return None;
        }
        self.last_emit_ms = ts_ms;
        let mut sorted: Vec<f32> = self.samples_ms.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Some(HealthMetrics {
            ts_ms,
            fps_in: self.in_count as f32,
            fps_out: self.out_count as f32,
            latency_p50_ms: percentile(&sorted, 0.50),
            latency_p95_ms: percentile(&sorted, 0.95),
            dropped_frames: self.dropped_frames,
        })
    }
}

fn percentile(values: &[f32], p: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let idx = ((values.len() as f32 - 1.0) * p).round() as usize;
    values[idx.min(values.len() - 1)]
}
