//! Island geometry and its spring animation.

use poni_core::geom::Rect;
use poni_core::spring::Spring;

/// Concave flare where the island meets the top screen edge.
pub const SHOULDER: f32 = 9.0;
/// Room around the island in the canvas. While expanded it is filled at
/// alpha 1/255 so it still catches the mouse (a forgiving edge for the big
/// panel); collapsed it stays fully transparent and clicks pass through.
/// SHOULDER + HIT_MARGIN is kept even (20) so the body edge lands on a whole
/// pixel at 150 % scaling.
pub const HIT_MARGIN: f32 = 11.0;

/// Spring tunings (stiffness, damping). Growing goes wide first, then drops:
/// height trails width a little. Shrinking is nearly critically damped so the
/// small pill doesn't squash below its size. Corners never wobble.
const GROW_W: (f32, f32) = (420.0, 30.0); // zeta 0.73
const GROW_H: (f32, f32) = (300.0, 26.0); // zeta 0.75
const SHRINK_W: (f32, f32) = (330.0, 33.0); // zeta 0.91
const SHRINK_H: (f32, f32) = (420.0, 38.0); // zeta 0.93
const RADIUS: (f32, f32) = (420.0, 41.0); // critical
/// Content opacity: critically damped, tau about 38 ms.
const CONTENT: (f32, f32) = (700.0, 52.9);
/// Delay before new content starts fading in (seconds).
const FADE_DELAY_GROW: f64 = 0.03;
const FADE_DELAY_SHRINK: f64 = 0.05;

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
    /// The body's distance below the top edge: 0 while it hangs from the
    /// edge, the gap while it floats (the Pill theme).
    top: f32,
    floating: bool,
}

impl Island {
    pub fn new(g: Geom) -> Self {
        Self {
            w: Spring::new(g.w, GROW_W.0, GROW_W.1),
            h: Spring::new(g.h, GROW_H.0, GROW_H.1),
            r: Spring::new(g.r, RADIUS.0, RADIUS.1),
            content: Spring::new(1.0, CONTENT.0, CONTENT.1),
            fade_in_at: None,
            target: g,
            top: 0.0,
            floating: false,
        }
    }

    /// Float `Some(gap)` DIPs below the top edge (the Pill theme), or hang
    /// from it (`None`).
    pub fn set_floating(&mut self, gap: Option<f32>) {
        self.floating = gap.is_some();
        self.top = gap.unwrap_or(0.0);
    }

    pub fn floating(&self) -> bool {
        self.floating
    }

    /// The body's distance below the top edge (DIPs).
    pub fn top(&self) -> f32 {
        self.top
    }

    pub fn target(&self) -> Geom {
        self.target
    }

    /// Animate towards `g`. When `new_content` is true the content cross-fades:
    /// the new content starts from transparent and fades in shortly after the
    /// outline starts moving (the app fades the old content out meanwhile).
    /// Springs keep their value and velocity; only their tuning changes with
    /// the direction of the morph.
    pub fn set_target(&mut self, g: Geom, now: f64, new_content: bool) {
        let growing = g.w * g.h > self.w.value * self.h.value;
        let (tw, th) = if growing { (GROW_W, GROW_H) } else { (SHRINK_W, SHRINK_H) };
        self.w.retune(tw.0, tw.1);
        self.h.retune(th.0, th.1);
        self.r.retune(RADIUS.0, RADIUS.1);
        self.target = g;
        self.w.set_target(g.w);
        self.h.set_target(g.h);
        self.r.set_target(g.r);
        if new_content {
            self.content.snap(0.0);
            self.fade_in_at = Some(now + if growing { FADE_DELAY_GROW } else { FADE_DELAY_SHRINK });
        }
    }

    /// The new content fades in at once from `alpha` (the same content coming
    /// back mid-fade), instead of from transparent after the usual delay.
    pub fn resume_content(&mut self, alpha: f32) {
        self.content.snap(alpha.clamp(0.0, 1.0));
        self.content.set_target(1.0);
        self.fade_in_at = None;
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

    /// The island body, hanging from the top edge or floating below it,
    /// centred on `cx` (DIPs).
    pub fn body_rect_at(&self, cx: f32) -> Rect {
        let g = self.current();
        Rect::new(cx - g.w / 2.0, self.top, g.w, g.h)
    }

    /// The island plus its shoulders (or, floating, the gap above it) and
    /// hit margin, centred on `cx` (DIPs).
    pub fn canvas_rect_at(&self, cx: f32) -> Rect {
        let g = self.current();
        let side = self.side_room();
        Rect::new(cx - g.w / 2.0 - side, 0.0, g.w + 2.0 * side, self.top + g.h + self.bottom_room())
    }

    /// Room the window keeps on each side of the body: the shoulders and
    /// hit margin, or (floating) the shadow's room.
    pub fn side_room(&self) -> f32 {
        if self.floating {
            crate::pill::shape::MARGIN
        } else {
            SHOULDER + HIT_MARGIN
        }
    }

    /// Room the window keeps below the body.
    pub fn bottom_room(&self) -> f32 {
        if self.floating {
            crate::pill::shape::MARGIN
        } else {
            HIT_MARGIN
        }
    }

    /// Where the resting island takes the mouse: its body, and while it
    /// floats the gap above it too (a pointer thrown at the top edge).
    pub fn hover_rect_at(&self, cx: f32) -> Rect {
        let body = self.body_rect_at(cx);
        if self.floating {
            crate::pill::shape::hover_rect(body)
        } else {
            body
        }
    }
}
