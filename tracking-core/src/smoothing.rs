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
        if !x.is_finite() {
            return if self.x_filter.initialized {
                self.x_filter.y
            } else {
                0.0
            };
        }
        let dt_s = if dt_s.is_finite() { dt_s } else { 1.0 / 30.0 };
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
    min_cutoff: f32,
    beta: f32,
    d_cutoff: f32,
}

impl OneEuroSmoother {
    pub fn new(n_points: usize, min_cutoff: f32, beta: f32, d_cutoff: f32) -> Self {
        let mut points = Vec::with_capacity(n_points);
        for _ in 0..n_points {
            points.push(OneEuroPoint::new(min_cutoff, beta, d_cutoff));
        }
        Self {
            points,
            min_cutoff,
            beta,
            d_cutoff,
        }
    }

    pub fn filter(&mut self, landmarks: &[Landmark], dt_s: f32) -> Vec<Landmark> {
        let mut out = Vec::with_capacity(landmarks.len());
        self.filter_into(landmarks, dt_s, &mut out);
        out
    }

    pub fn filter_into(&mut self, landmarks: &[Landmark], dt_s: f32, out: &mut Vec<Landmark>) {
        self.ensure_points(landmarks.len());
        out.clear();
        out.reserve(landmarks.len());
        out.extend(
            landmarks
                .iter()
                .enumerate()
                .map(|(i, lm)| self.points[i].filter(*lm, dt_s)),
        );
    }

    fn ensure_points(&mut self, n_points: usize) {
        if n_points <= self.points.len() {
            return;
        }
        self.points.reserve(n_points - self.points.len());
        while self.points.len() < n_points {
            self.points
                .push(OneEuroPoint::new(self.min_cutoff, self.beta, self.d_cutoff));
        }
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

    #[test]
    fn grows_when_more_points_are_filtered_than_initially_configured() {
        let mut smooth = OneEuroSmoother::new(1, 1.0, 0.02, 1.0);
        let out = smooth.filter(
            &[
                Landmark {
                    x: 0.1,
                    y: 0.2,
                    z: 0.0,
                },
                Landmark {
                    x: 0.3,
                    y: 0.4,
                    z: 0.0,
                },
            ],
            1.0 / 30.0,
        );

        assert_eq!(out.len(), 2);
        assert_eq!(out[1].x, 0.3);
    }

    #[test]
    fn filter_into_reuses_caller_buffer() {
        let mut smooth = OneEuroSmoother::new(2, 1.0, 0.02, 1.0);
        let landmarks = [
            Landmark {
                x: 0.1,
                y: 0.2,
                z: 0.0,
            },
            Landmark {
                x: 0.3,
                y: 0.4,
                z: 0.0,
            },
        ];
        let mut out = Vec::with_capacity(2);
        let ptr = out.as_ptr();

        smooth.filter_into(&landmarks, 1.0 / 30.0, &mut out);

        assert_eq!(out.len(), 2);
        assert_eq!(out.as_ptr(), ptr);
        assert_eq!(out[0].x, 0.1);
        assert_eq!(out[1].x, 0.3);
    }

    #[test]
    fn non_finite_samples_do_not_poison_filter_state() {
        let mut smooth = OneEuroSmoother::new(1, 1.0, 0.02, 1.0);
        let first = smooth.filter(
            &[Landmark {
                x: 0.25,
                y: 0.5,
                z: 0.0,
            }],
            1.0 / 30.0,
        )[0];
        let bad = smooth.filter(
            &[Landmark {
                x: f32::NAN,
                y: f32::INFINITY,
                z: f32::NEG_INFINITY,
            }],
            f32::NAN,
        )[0];
        let next = smooth.filter(
            &[Landmark {
                x: 0.25,
                y: 0.5,
                z: 0.0,
            }],
            1.0 / 30.0,
        )[0];

        assert_eq!(bad, first);
        assert!(next.x.is_finite());
        assert!(next.y.is_finite());
        assert!(next.z.is_finite());
    }
}
