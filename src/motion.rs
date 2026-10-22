//! Springs: motion that runs a little past where it is going and settles back, as something
//! flicked does. Everything is worked out from the time, so a frame drawn late lands where it
//! would have been, and tests can ask for any moment.

use std::f32::consts::TAU;
use std::time::Instant;

/// How a spring moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feel {
    /// About how long one swing takes, in seconds.
    pub response: f32,
    /// Below 1 it overshoots, the more the lower; at 1 it doesn't.
    pub damping: f32,
}

/// Something appearing: the wheel opening, wedges unfolding. Overshoots by about 8 %.
pub const POP: Feel = Feel {
    response: 0.36,
    damping: 0.62,
};

/// A wedge swelling toward the pointer, and back. Overshoots by about 16 %.
pub const SWELL: Feel = Feel {
    response: 0.3,
    damping: 0.5,
};

impl Feel {
    /// Where a spring started at `from` with `velocity`, heading for `to`, is `t` seconds
    /// later, and how fast it moves there.
    fn at(self, from: f32, velocity: f32, to: f32, t: f32) -> (f32, f32) {
        if t <= 0.0 {
            return (from, velocity);
        }
        let w = TAU / self.response;
        let z = self.damping.clamp(0.05, 1.0);
        let a = from - to;
        let e = (-z * w * t).exp();
        if z >= 1.0 {
            let b = velocity + w * a;
            return (to + e * (a + b * t), e * (b - w * (a + b * t)));
        }
        let wd = w * (1.0 - z * z).sqrt();
        let b = (velocity + z * w * a) / wd;
        let (sin, cos) = (wd * t).sin_cos();
        let x = to + e * (a * cos + b * sin);
        let v = e * ((b * wd - z * w * a) * cos - (a * wd + z * w * b) * sin);
        (x, v)
    }

    /// 0 → 1, running a little past 1 on the way, `t` seconds in (0 before it starts).
    pub fn step(self, t: f32) -> f32 {
        if t <= 0.0 { 0.0 } else { self.at(0.0, 0.0, 1.0, t).0 }
    }

    /// Where a spring at rest is, `t` seconds after a flick sends it out: out and back, past
    /// where it rested and back again, settling there. Its first swing reaches about 0.6.
    pub fn kick(self, t: f32) -> f32 {
        if t <= 0.0 { 0.0 } else { self.at(0.0, TAU / self.response, 0.0, t).0 }
    }

    /// How long until it is within a hundredth of the way it had to go.
    pub fn settle(self) -> f32 {
        let z = self.damping.clamp(0.05, 1.0);
        if z >= 1.0 {
            // Critically damped, it falls off as e^(-wt)·(1 + wt).
            6.64 * self.response / TAU
        } else {
            4.6 * self.response / (TAU * z)
        }
    }
}

/// The hub flicked toward a click, and back. Overshoots, coming back, by about 20 %.
pub const NUDGE: Feel = Feel {
    response: 0.26,
    damping: 0.45,
};

/// A value that springs toward wherever it is sent, without a jump when sent elsewhere halfway.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    feel: Feel,
    from: f32,
    velocity: f32,
    to: f32,
    since: Instant,
}

impl Spring {
    /// At rest at `value`.
    pub fn new(feel: Feel, value: f32, now: Instant) -> Spring {
        Spring {
            feel,
            from: value,
            velocity: 0.0,
            to: value,
            since: now,
        }
    }

    fn elapsed(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.since).as_secs_f32()
    }

    pub fn value(&self, now: Instant) -> f32 {
        if !self.moving(now) {
            return self.to;
        }
        let t = self.elapsed(now);
        self.feel.at(self.from, self.velocity, self.to, t).0
    }

    /// Sends it toward `to` from where it is now, keeping its speed.
    pub fn set(&mut self, to: f32, now: Instant) {
        if to == self.to {
            return;
        }
        let t = self.elapsed(now);
        let (from, velocity) = if self.moving(now) {
            self.feel.at(self.from, self.velocity, self.to, t)
        } else {
            (self.to, 0.0)
        };
        *self = Spring {
            from,
            velocity,
            to,
            since: now,
            ..*self
        };
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    /// Whether it still moves enough to be seen.
    pub fn moving(&self, now: Instant) -> bool {
        self.elapsed(now) < self.feel.settle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_pop_runs_past_and_settles() {
        assert_eq!(POP.step(0.0), 0.0);
        let peak = (1..100)
            .map(|i| POP.step(i as f32 / 100.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.04 && peak < 1.12, "{peak}");
        assert!((POP.step(POP.settle()) - 1.0).abs() < 0.01);
        assert!((POP.step(5.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn its_speed_is_how_fast_it_moves_and_it_settles_in_time() {
        for feel in [SWELL, POP, Feel { response: 0.3, damping: 1.0 }] {
            for t in [0.02, 0.1, 0.25] {
                let dt = 1e-3;
                let ahead = feel.at(0.3, 2.0, 1.0, t + dt).0;
                let behind = feel.at(0.3, 2.0, 1.0, t - dt).0;
                let speed = feel.at(0.3, 2.0, 1.0, t).1;
                assert!(((ahead - behind) / (2.0 * dt) - speed).abs() < 0.05, "{feel:?} {t}");
            }
            assert!((feel.step(feel.settle()) - 1.0).abs() < 0.011, "{feel:?}");
        }
    }

    #[test]
    fn sent_elsewhere_halfway_it_carries_on_from_where_it_was() {
        let t0 = Instant::now();
        let mut s = Spring::new(SWELL, 0.0, t0);
        s.set(1.0, t0);
        let half = t0 + Duration::from_millis(60);
        let before = s.value(half);
        assert!(before > 0.1 && before < 1.0, "{before}");
        s.set(0.0, half);
        assert!((s.value(half) - before).abs() < 1e-5);
        // Still rising for a moment: it had speed.
        assert!(s.value(half + Duration::from_millis(10)) > before);
        let late = half + Duration::from_secs(2);
        assert!(!s.moving(late));
        assert_eq!(s.value(late), 0.0);
    }
}
