//! Damped spring used for every island animation (size, corner radius,
//! content opacity). Springs keep momentum when the target changes mid-flight,
//! which is what makes the island feel "alive" rather than tweened.

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Spring {
    pub fn new(value: f32, stiffness: f32, damping: f32) -> Self {
        Self { value, velocity: 0.0, target: value, stiffness, damping }
    }

    /// A slightly under-damped spring (small overshoot), good for the island outline.
    pub fn bouncy(value: f32) -> Self {
        Self::new(value, 420.0, 30.0)
    }

    /// A critically damped spring (no overshoot), good for opacity.
    pub fn smooth(value: f32) -> Self {
        let k = 380.0f32;
        Self::new(value, k, 2.0 * k.sqrt())
    }

    /// Change stiffness/damping in flight, keeping value and velocity.
    pub fn retune(&mut self, stiffness: f32, damping: f32) {
        self.stiffness = stiffness;
        self.damping = damping;
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Jump to `value` with no motion.
    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    /// Advance by `dt` seconds (semi-implicit Euler with fixed sub-steps).
    pub fn step(&mut self, dt: f32) {
        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
            return;
        }
        let mut remaining = dt.clamp(0.0, 0.1);
        const H: f32 = 1.0 / 240.0;
        while remaining > 0.0 {
            let h = remaining.min(H);
            let force = -self.stiffness * (self.value - self.target) - self.damping * self.velocity;
            self.velocity += force * h;
            self.value += self.velocity * h;
            remaining -= h;
        }
        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }

    /// Settled when within 0.05 units of the target and nearly still. For
    /// opacity-style springs (0..1) callers should use [`Spring::is_settled_eps`].
    pub fn is_settled(&self) -> bool {
        self.is_settled_eps(0.05)
    }

    pub fn is_settled_eps(&self, eps: f32) -> bool {
        (self.value - self.target).abs() < eps && self.velocity.abs() < eps * 4.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converges_to_target() {
        let mut s = Spring::bouncy(0.0);
        s.set_target(100.0);
        let mut t = 0.0;
        while !s.is_settled() && t < 3.0 {
            s.step(1.0 / 60.0);
            t += 1.0 / 60.0;
        }
        assert!(s.is_settled(), "spring did not settle within 3s");
        assert_eq!(s.value, 100.0);
        assert!(t < 1.2, "spring too slow: {t}s");
    }

    #[test]
    fn bouncy_overshoots_a_little() {
        let mut s = Spring::bouncy(0.0);
        s.set_target(100.0);
        let mut max: f32 = 0.0;
        for _ in 0..180 {
            s.step(1.0 / 60.0);
            max = max.max(s.value);
        }
        assert!(max > 100.0 && max < 115.0, "overshoot {max}");
    }

    #[test]
    fn smooth_never_overshoots() {
        let mut s = Spring::smooth(0.0);
        s.set_target(1.0);
        for _ in 0..240 {
            s.step(1.0 / 60.0);
            assert!(s.value <= 1.0 + 1e-3);
        }
    }

    #[test]
    fn large_dt_is_stable() {
        let mut s = Spring::bouncy(0.0);
        s.set_target(50.0);
        s.step(5.0); // clamped to 0.1s internally
        assert!(s.value.is_finite());
    }
}
