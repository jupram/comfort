use crate::types::HealthMetrics;
use std::collections::VecDeque;

pub struct HealthTracker {
    samples_ms: VecDeque<f32>,
    sorted_samples_ms: Vec<f32>,
    window_in_count: u64,
    window_out_count: u64,
    dropped_frames: u64,
    last_emit_ms: u64,
}

impl HealthTracker {
    pub fn new() -> Self {
        Self {
            samples_ms: VecDeque::new(),
            sorted_samples_ms: Vec::new(),
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
        if latency_ms.is_finite() {
            self.samples_ms.push_back(latency_ms.max(0.0));
        }
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
        self.sorted_samples_ms.clear();
        self.sorted_samples_ms
            .extend(self.samples_ms.iter().copied());
        self.sorted_samples_ms
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let metrics = HealthMetrics {
            ts_ms,
            fps_in: self.window_in_count as f32 / elapsed_s,
            fps_out: self.window_out_count as f32 / elapsed_s,
            latency_p50_ms: percentile(&self.sorted_samples_ms, 0.50),
            latency_p95_ms: percentile(&self.sorted_samples_ms, 0.95),
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

    #[test]
    fn ignores_non_finite_latency_samples() {
        let mut tracker = HealthTracker::new();

        tracker.on_frame(f32::NAN, true, false);
        tracker.on_frame(f32::INFINITY, true, false);
        tracker.on_frame(-2.0, true, false);
        tracker.on_frame(2.0, true, false);
        tracker.on_frame(6.0, true, false);

        let metrics = tracker.maybe_emit(1_000).expect("metrics");

        assert_eq!(metrics.fps_in, 5.0);
        assert_eq!(metrics.latency_p50_ms, 2.0);
        assert_eq!(metrics.latency_p95_ms, 6.0);
    }

    #[test]
    fn reuses_sorted_sample_buffer_between_emits() {
        let mut tracker = HealthTracker::new();
        for i in 0..240 {
            tracker.on_frame(i as f32, true, false);
        }
        let _ = tracker.maybe_emit(1_000).expect("first metrics");
        let ptr = tracker.sorted_samples_ms.as_ptr();
        let capacity = tracker.sorted_samples_ms.capacity();

        for i in 0..16 {
            tracker.on_frame(i as f32, true, false);
        }
        let _ = tracker.maybe_emit(2_000).expect("second metrics");

        assert_eq!(tracker.sorted_samples_ms.as_ptr(), ptr);
        assert_eq!(tracker.sorted_samples_ms.capacity(), capacity);
    }
}
