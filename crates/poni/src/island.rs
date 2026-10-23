//! Island geometry and its spring animation.

use poni_core::geom::Rect;
use poni_core::spring::Spring;

/// Concave flare where the island meets the top screen edge.
pub const SHOULDER: f32 = 9.0;
/// Near-invisible margin around the island that still catches the mouse
/// (alpha 1/255), so the hover target is a bit larger than what you see.
pub const HIT_MARGIN: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geom {
    pub w: f32,
    pub h: f32,
    /// Bottom corner radius.
    pub r: f32,
}

impl Geom {
    pub const fn new(w: f32, h: f32, r: f32) -> Self {
        Self { w, h, r }
    }
}

pub struct Island {
    w: Spring,
    h: Spring,
    r: Spring,
    content: Spring,
    /// When set, content fades in once `now` passes this time.
    fade_in_at: Option<f64>,
    target: Geom,
}

impl Island {
    pub fn new(g: Geom) -> Self {
        Self {
            w: Spring::bouncy(g.w),
            h: Spring::bouncy(g.h),
            r: Spring::bouncy(g.r),
            content: Spring::smooth(1.0),
            fade_in_at: None,
            target: g,
        }
    }

    pub fn target(&self) -> Geom {
        self.target
    }

    /// Animate towards `g`. When `new_content` is true the content cross-fades:
    /// old content disappears at once and the new content fades in shortly
    /// after the outline starts moving.
    pub fn set_target(&mut self, g: Geom, now: f64, new_content: bool) {
        self.target = g;
        self.w.set_target(g.w);
        self.h.set_target(g.h);
        self.r.set_target(g.r);
        if new_content {
            self.content.snap(0.0);
            let growing = g.w * g.h > self.w.value * self.h.value;
            self.fade_in_at = Some(now + if growing { 0.09 } else { 0.05 });
        }
    }

    /// Jump straight to `g` without animating.
    pub fn snap(&mut self, g: Geom) {
        self.target = g;
        self.w.snap(g.w);
        self.h.snap(g.h);
        self.r.snap(g.r);
        self.content.snap(1.0);
        self.fade_in_at = None;
    }

    pub fn step(&mut self, dt: f32, now: f64) {
        self.w.step(dt);
        self.h.step(dt);
        self.r.step(dt);
        if let Some(t) = self.fade_in_at {
            if now >= t {
                self.content.set_target(1.0);
                self.fade_in_at = None;
            }
        }
        self.content.step(dt);
    }

    pub fn settled(&self) -> bool {
        self.fade_in_at.is_none()
            && self.w.is_settled()
            && self.h.is_settled()
            && self.r.is_settled()
            && self.content.is_settled_eps(0.002)
    }

    pub fn current(&self) -> Geom {
        Geom { w: self.w.value.max(20.0), h: self.h.value.max(8.0), r: self.r.value.max(0.0) }
    }

    pub fn content_alpha(&self) -> f32 {
        self.content.value.clamp(0.0, 1.0)
    }

    /// Full canvas size in DIPs (island + shoulders + hit margin).
    pub fn canvas_size(&self) -> (f32, f32) {
        let g = self.current();
        (g.w + 2.0 * (SHOULDER + HIT_MARGIN), g.h + HIT_MARGIN)
    }

    /// The island body inside the canvas (DIPs).
    pub fn body_rect(&self) -> Rect {
        let g = self.current();
        Rect::new(SHOULDER + HIT_MARGIN, 0.0, g.w, g.h)
    }
}
