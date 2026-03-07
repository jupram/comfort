use crate::types::HealthEvent;
use std::collections::VecDeque;

pub struct HealthTracker {
    latency_ms: VecDeque<f32>,
    in_count: u64,
    out_count: u64,
    dropped_frames: u64,
    last_emit_ms: u64,
}

impl HealthTracker {
    pub fn new() -> Self {
        Self {
            latency_ms: VecDeque::new(),
            in_count: 0,
            out_count: 0,
            dropped_frames: 0,
            last_emit_ms: 0,
        }
    }

    pub fn on_frame(&mut self, latency_ms: f32, had_output: bool, dropped: bool) {
        self.in_count += 1;
        if had_output {
            self.out_count += 1;
        }
        if dropped {
            self.dropped_frames += 1;
        }
        self.latency_ms.push_back(latency_ms);
        while self.latency_ms.len() > 240 {
            let _ = self.latency_ms.pop_front();
        }
    }

    pub fn maybe_emit(&mut self, ts_ms: u64) -> Option<HealthEvent> {
        if self.last_emit_ms != 0 && ts_ms.saturating_sub(self.last_emit_ms) < 1000 {
            return None;
        }
        self.last_emit_ms = ts_ms;

        let mut sorted: Vec<f32> = self.latency_ms.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p50 = percentile(&sorted, 0.50);
        let p95 = percentile(&sorted, 0.95);

        Some(HealthEvent {
            ts_ms,
            fps_in: self.in_count as f32,
            fps_out: self.out_count as f32,
            latency_ms_p50: p50,
            latency_ms_p95: p95,
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
