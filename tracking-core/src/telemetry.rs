use crate::types::HealthMetrics;
use std::collections::VecDeque;

pub struct HealthTracker {
    samples_ms: VecDeque<f32>,
    window_in_count: u64,
    window_out_count: u64,
    dropped_frames: u64,
    last_emit_ms: u64,
}

impl HealthTracker {
    pub fn new() -> Self {
        Self {
            samples_ms: VecDeque::new(),
            window_in_count: 0,
            window_out_count: 0,
            dropped_frames: 0,
            last_emit_ms: 0,
        }
    }

    pub fn on_frame(&mut self, latency_ms: f32, emitted_output: bool, dropped: bool) {
        self.window_in_count += 1;
        if emitted_output {
            self.window_out_count += 1;
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
        let elapsed_s = if self.last_emit_ms == 0 {
            1.0
        } else {
            (ts_ms.saturating_sub(self.last_emit_ms) as f32 / 1000.0).max(0.001)
        };
        self.last_emit_ms = ts_ms;
        let mut sorted: Vec<f32> = self.samples_ms.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let metrics = HealthMetrics {
            ts_ms,
            fps_in: self.window_in_count as f32 / elapsed_s,
            fps_out: self.window_out_count as f32 / elapsed_s,
            latency_p50_ms: percentile(&sorted, 0.50),
            latency_p95_ms: percentile(&sorted, 0.95),
            dropped_frames: self.dropped_frames,
        };
        self.window_in_count = 0;
        self.window_out_count = 0;
        Some(metrics)
    }
}

impl Default for HealthTracker {
    fn default() -> Self {
        Self::new()
    }
}

fn percentile(values: &[f32], p: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let idx = ((values.len() as f32 - 1.0) * p).round() as usize;
    values[idx.min(values.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_rates_for_emit_window_not_lifetime_counts() {
        let mut tracker = HealthTracker::new();

        for _ in 0..30 {
            tracker.on_frame(2.0, true, false);
        }
        let first = tracker.maybe_emit(1_000).expect("first metrics");
        assert_eq!(first.fps_in, 30.0);
        assert_eq!(first.fps_out, 30.0);

        for _ in 0..15 {
            tracker.on_frame(4.0, false, false);
        }
        let second = tracker.maybe_emit(2_000).expect("second metrics");
        assert_eq!(second.fps_in, 15.0);
        assert_eq!(second.fps_out, 0.0);
    }
}
