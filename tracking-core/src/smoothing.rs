use crate::types::Landmark;

#[derive(Debug, Clone, Copy)]
struct LowPassFilter {
    y: f32,
    initialized: bool,
}

impl LowPassFilter {
    fn filter(&mut self, x: f32, alpha: f32) -> f32 {
        if !self.initialized {
            self.y = x;
            self.initialized = true;
            return x;
        }
        self.y = alpha * x + (1.0 - alpha) * self.y;
        self.y
    }
}

#[derive(Debug, Clone, Copy)]
struct OneEuroAxis {
    min_cutoff: f32,
    beta: f32,
    d_cutoff: f32,
    x_filter: LowPassFilter,
    dx_filter: LowPassFilter,
    prev_x: f32,
    has_prev: bool,
}

impl OneEuroAxis {
    fn new(min_cutoff: f32, beta: f32, d_cutoff: f32) -> Self {
        Self {
            min_cutoff,
            beta,
            d_cutoff,
            x_filter: LowPassFilter {
                y: 0.0,
                initialized: false,
            },
            dx_filter: LowPassFilter {
                y: 0.0,
                initialized: false,
            },
            prev_x: 0.0,
            has_prev: false,
        }
    }

    fn alpha(cutoff: f32, dt_s: f32) -> f32 {
        let tau = 1.0 / (2.0 * std::f32::consts::PI * cutoff.max(1e-4));
        1.0 / (1.0 + tau / dt_s.max(1e-4))
    }

    fn filter(&mut self, x: f32, dt_s: f32) -> f32 {
        let dx = if self.has_prev {
            (x - self.prev_x) / dt_s.max(1e-4)
        } else {
            0.0
        };
        self.prev_x = x;
        self.has_prev = true;

        let a_d = Self::alpha(self.d_cutoff, dt_s);
        let dx_hat = self.dx_filter.filter(dx, a_d);
        let cutoff = self.min_cutoff + self.beta * dx_hat.abs();
        let a = Self::alpha(cutoff, dt_s);
        self.x_filter.filter(x, a)
    }
}

#[derive(Debug, Clone)]
struct OneEuroPoint {
    x: OneEuroAxis,
    y: OneEuroAxis,
    z: OneEuroAxis,
}

impl OneEuroPoint {
    fn new(min_cutoff: f32, beta: f32, d_cutoff: f32) -> Self {
        Self {
            x: OneEuroAxis::new(min_cutoff, beta, d_cutoff),
            y: OneEuroAxis::new(min_cutoff, beta, d_cutoff),
            z: OneEuroAxis::new(min_cutoff, beta, d_cutoff),
        }
    }

    fn filter(&mut self, p: Landmark, dt_s: f32) -> Landmark {
        Landmark {
            x: self.x.filter(p.x, dt_s),
            y: self.y.filter(p.y, dt_s),
            z: self.z.filter(p.z, dt_s),
        }
    }
}

pub struct OneEuroSmoother {
    points: Vec<OneEuroPoint>,
}

impl OneEuroSmoother {
    pub fn new(n_points: usize, min_cutoff: f32, beta: f32, d_cutoff: f32) -> Self {
        let mut points = Vec::with_capacity(n_points);
        for _ in 0..n_points {
            points.push(OneEuroPoint::new(min_cutoff, beta, d_cutoff));
        }
        Self { points }
    }

    pub fn filter(&mut self, landmarks: &[Landmark], dt_s: f32) -> Vec<Landmark> {
        landmarks
            .iter()
            .enumerate()
            .map(|(i, lm)| self.points[i].filter(*lm, dt_s))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_euro_converges() {
        let mut smooth = OneEuroSmoother::new(1, 1.0, 0.02, 1.0);
        let mut out = 0.0;
        for _ in 0..20 {
            out = smooth.filter(
                &[Landmark {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                }],
                1.0 / 30.0,
            )[0]
            .x;
        }
        assert!(out > 0.8);
        assert!(out <= 1.0);
    }
}
