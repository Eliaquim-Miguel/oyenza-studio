//! A small SVG path-data parser (`M L H V C S Q T A Z`, absolute and
//! relative). Icons are stored as SVG path strings in a 24×24 box and turned
//! into Direct2D geometry by the Windows crate.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    QuadTo(f32, f32, f32, f32),
    /// rx, ry, x-axis rotation (degrees), large_arc, sweep (true = clockwise in y-down), end x, end y
    ArcTo(f32, f32, f32, bool, bool, f32, f32),
    Close,
}

#[derive(Debug, PartialEq)]
pub struct ParseError(pub String);

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',') {
            self.i += 1;
        }
    }
    fn peek_cmd(&mut self) -> Option<u8> {
        self.skip_ws();
        self.s.get(self.i).copied().filter(|c| c.is_ascii_alphabetic())
    }
    fn at_number(&mut self) -> bool {
        self.skip_ws();
        matches!(self.s.get(self.i), Some(c) if c.is_ascii_digit() || *c == b'-' || *c == b'+' || *c == b'.')
    }
    fn number(&mut self) -> Result<f32, ParseError> {
        self.skip_ws();
        let start = self.i;
        if matches!(self.s.get(self.i), Some(b'-') | Some(b'+')) {
            self.i += 1;
        }
        let mut seen_dot = false;
        let mut seen_digit = false;
        while let Some(&c) = self.s.get(self.i) {
            if c.is_ascii_digit() {
                seen_digit = true;
                self.i += 1;
            } else if c == b'.' && !seen_dot {
                seen_dot = true;
                self.i += 1;
            } else {
                break;
            }
        }
        if matches!(self.s.get(self.i), Some(b'e') | Some(b'E')) {
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'-') | Some(b'+')) {
                self.i += 1;
            }
            while matches!(self.s.get(self.i), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if !seen_digit {
            return Err(ParseError(format!("expected number at byte {start}")));
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()
            .and_then(|t| t.parse::<f32>().ok())
            .ok_or_else(|| ParseError(format!("bad number at byte {start}")))
    }
    /// Arc flags may be written without separators ("a1 1 0 01 2 2").
    fn flag(&mut self) -> Result<bool, ParseError> {
        self.skip_ws();
        match self.s.get(self.i) {
            Some(b'0') => {
                self.i += 1;
                Ok(false)
            }
            Some(b'1') => {
                self.i += 1;
                Ok(true)
            }
            _ => Err(ParseError(format!("expected arc flag at byte {}", self.i))),
        }
    }
}

/// Parse path data into absolute segments. Smooth curves (S/T) are expanded
/// into full cubic/quadratic segments.
pub fn parse(d: &str) -> Result<Vec<Seg>, ParseError> {
    let mut lx = Lexer { s: d.as_bytes(), i: 0 };
    let mut out = Vec::new();
    let (mut cx, mut cy) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let mut last_ctrl: Option<(f32, f32, bool)> = None; // (x, y, is_cubic)
    let mut cmd: Option<u8> = None;

    loop {
        if let Some(c) = lx.peek_cmd() {
            lx.i += 1;
            cmd = Some(c);
        } else if !lx.at_number() {
            break;
        }
        let Some(c) = cmd else { return Err(ParseError("path must start with a command".into())) };
        let rel = c.is_ascii_lowercase();
        let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
        match c.to_ascii_uppercase() {
            b'M' => {
                let (x, y) = (lx.number()? + ox, lx.number()? + oy);
                out.push(Seg::MoveTo(x, y));
                cx = x;
                cy = y;
                sx = x;
                sy = y;
                last_ctrl = None;
                // Subsequent pairs are implicit LineTo.
                cmd = Some(if rel { b'l' } else { b'L' });
            }
            b'L' => {
                let (x, y) = (lx.number()? + ox, lx.number()? + oy);
                out.push(Seg::LineTo(x, y));
                cx = x;
                cy = y;
                last_ctrl = None;
            }
            b'H' => {
                let x = lx.number()? + ox;
                out.push(Seg::LineTo(x, cy));
                cx = x;
                last_ctrl = None;
            }
            b'V' => {
                let y = lx.number()? + oy;
                out.push(Seg::LineTo(cx, y));
                cy = y;
                last_ctrl = None;
            }
            b'C' => {
                let (x1, y1) = (lx.number()? + ox, lx.number()? + oy);
                let (x2, y2) = (lx.number()? + ox, lx.number()? + oy);
                let (x, y) = (lx.number()? + ox, lx.number()? + oy);
                out.push(Seg::CubicTo(x1, y1, x2, y2, x, y));
                last_ctrl = Some((x2, y2, true));
                cx = x;
                cy = y;
            }
            b'S' => {
                let (x1, y1) = match last_ctrl {
                    Some((lx_, ly_, true)) => (2.0 * cx - lx_, 2.0 * cy - ly_),
                    _ => (cx, cy),
                };
                let (x2, y2) = (lx.number()? + ox, lx.number()? + oy);
                let (x, y) = (lx.number()? + ox, lx.number()? + oy);
                out.push(Seg::CubicTo(x1, y1, x2, y2, x, y));
                last_ctrl = Some((x2, y2, true));
                cx = x;
                cy = y;
            }
            b'Q' => {
                let (x1, y1) = (lx.number()? + ox, lx.number()? + oy);
                let (x, y) = (lx.number()? + ox, lx.number()? + oy);
                out.push(Seg::QuadTo(x1, y1, x, y));
                last_ctrl = Some((x1, y1, false));
                cx = x;
                cy = y;
            }
            b'T' => {
                let (x1, y1) = match last_ctrl {
                    Some((lx_, ly_, false)) => (2.0 * cx - lx_, 2.0 * cy - ly_),
                    _ => (cx, cy),
                };
                let (x, y) = (lx.number()? + ox, lx.number()? + oy);
                out.push(Seg::QuadTo(x1, y1, x, y));
                last_ctrl = Some((x1, y1, false));
                cx = x;
                cy = y;
            }
            b'A' => {
                let rx = lx.number()?.abs();
                let ry = lx.number()?.abs();
                let rot = lx.number()?;
                let large = lx.flag()?;
                let sweep = lx.flag()?;
                let (x, y) = (lx.number()? + ox, lx.number()? + oy);
                out.push(Seg::ArcTo(rx, ry, rot, large, sweep, x, y));
                cx = x;
                cy = y;
                last_ctrl = None;
            }
            b'Z' => {
                out.push(Seg::Close);
                cx = sx;
                cy = sy;
                last_ctrl = None;
                cmd = None;
            }
            other => return Err(ParseError(format!("unsupported command '{}'", other as char))),
        }
    }
    Ok(out)
}

/// Convert an SVG elliptical arc (endpoint parameterisation) into cubic
/// Bézier segments `[c1x, c1y, c2x, c2y, x, y]`, at most 90° each.
///
/// Renderers differ in how well they draw native arcs (Wine's Direct2D, for
/// one, draws them as straight chords), so Poni always flattens arcs into
/// Béziers itself. Follows SVG 1.1 appendix F.6.5/F.6.6. `sweep = true` is
/// the positive-angle direction (clockwise on a y-down screen).
#[allow(clippy::too_many_arguments)]
pub fn arc_to_cubics(x1: f32, y1: f32, rx: f32, ry: f32, rot_deg: f32, large: bool, sweep: bool, x2: f32, y2: f32) -> Vec<[f32; 6]> {
    let (x1d, y1d, x2d, y2d) = (x1 as f64, y1 as f64, x2 as f64, y2 as f64);
    if (x1d - x2d).abs() < 1e-9 && (y1d - y2d).abs() < 1e-9 {
        return Vec::new();
    }
    let (mut rx, mut ry) = ((rx as f64).abs(), (ry as f64).abs());
    if rx < 1e-9 || ry < 1e-9 {
        // Degenerate radius: a straight line (as a cubic with collinear controls).
        return vec![[x1, y1, x2, y2, x2, y2]];
    }
    let phi = (rot_deg as f64).to_radians();
    let (sin_p, cos_p) = phi.sin_cos();
    let dx = (x1d - x2d) / 2.0;
    let dy = (y1d - y2d) / 2.0;
    let x1p = cos_p * dx + sin_p * dy;
    let y1p = -sin_p * dx + cos_p * dy;
    // Scale radii up if the arc can't reach the endpoint.
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut coef = if den.abs() < 1e-12 { 0.0 } else { (num / den).max(0.0).sqrt() };
    if large == sweep {
        coef = -coef;
    }
    let cxp = coef * (rx * y1p / ry);
    let cyp = coef * (-ry * x1p / rx);
    let cx = cos_p * cxp - sin_p * cyp + (x1d + x2d) / 2.0;
    let cy = sin_p * cxp + cos_p * cyp + (y1d + y2d) / 2.0;

    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| -> f64 {
        let dot = ux * vx + uy * vy;
        let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        let mut a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = angle(1.0, 0.0, ux, uy);
    let mut delta = angle(ux, uy, vx, vy);
    let tau = std::f64::consts::TAU;
    if !sweep && delta > 0.0 {
        delta -= tau;
    } else if sweep && delta < 0.0 {
        delta += tau;
    }

    let n = ((delta.abs() / std::f64::consts::FRAC_PI_2) - 1e-9).ceil().max(1.0) as usize;
    let step = delta / n as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let map = |ex: f64, ey: f64| -> (f32, f32) {
        let x = rx * ex;
        let y = ry * ey;
        ((cos_p * x - sin_p * y + cx) as f32, (sin_p * x + cos_p * y + cy) as f32)
    };
    let mut out = Vec::with_capacity(n);
    let mut t = theta1;
    for i in 0..n {
        let t2 = t + step;
        let (s1, c1) = t.sin_cos();
        let (s2, c2) = t2.sin_cos();
        let p1 = map(c1 - k * s1, s1 + k * c1);
        let p2 = map(c2 + k * s2, s2 - k * c2);
        // Land exactly on the requested endpoint.
        let p3 = if i + 1 == n { (x2, y2) } else { map(c2, s2) };
        out.push([p1.0, p1.1, p2.0, p2.1, p3.0, p3.1]);
        t = t2;
    }
    out
}

/// Convenience: SVG `<circle>` as path data (two arcs).
pub fn circle(cx: f32, cy: f32, r: f32) -> String {
    format!("M{} {}a{r} {r} 0 1 0 {} 0a{r} {r} 0 1 0 {} 0", cx - r, cy, 2.0 * r, -2.0 * r)
}

/// Convenience: SVG `<rect>` with corner radius as path data.
pub fn rect(x: f32, y: f32, w: f32, h: f32, rx: f32) -> String {
    let r = rx.min(w / 2.0).min(h / 2.0);
    if r <= 0.0 {
        return format!("M{x} {y}h{w}v{h}h{}z", -w);
    }
    format!(
        "M{} {y}h{}a{r} {r} 0 0 1 {r} {r}v{}a{r} {r} 0 0 1 {} {r}h{}a{r} {r} 0 0 1 {} {}v{}a{r} {r} 0 0 1 {r} {}z",
        x + r,
        w - 2.0 * r,
        h - 2.0 * r,
        -r,
        -(w - 2.0 * r),
        -r,
        -r,
        -(h - 2.0 * r),
        -r
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_commands() {
        let segs = parse("M2 3 L4 5 H10 V1 Z").unwrap();
        assert_eq!(segs, vec![Seg::MoveTo(2.0, 3.0), Seg::LineTo(4.0, 5.0), Seg::LineTo(10.0, 5.0), Seg::LineTo(10.0, 1.0), Seg::Close]);
    }

    #[test]
    fn relative_and_implicit_lineto() {
        let segs = parse("m1 1 2 2 l1-1").unwrap();
        assert_eq!(segs, vec![Seg::MoveTo(1.0, 1.0), Seg::LineTo(3.0, 3.0), Seg::LineTo(4.0, 2.0)]);
    }

    #[test]
    fn compact_numbers() {
        let segs = parse("M.5.5L-1-1.5").unwrap();
        assert_eq!(segs, vec![Seg::MoveTo(0.5, 0.5), Seg::LineTo(-1.0, -1.5)]);
    }

    #[test]
    fn arcs_with_packed_flags() {
        let segs = parse("M0 0a1 1 0 01 2 2").unwrap();
        assert_eq!(segs[1], Seg::ArcTo(1.0, 1.0, 0.0, false, true, 2.0, 2.0));
    }

    #[test]
    fn smooth_cubic_reflects() {
        let segs = parse("M0 0 C1 1 2 1 3 0 S5 -1 6 0").unwrap();
        assert_eq!(segs[2], Seg::CubicTo(4.0, -1.0, 5.0, -1.0, 6.0, 0.0));
    }

    #[test]
    fn helpers_parse() {
        assert!(parse(&circle(12.0, 12.0, 10.0)).unwrap().len() == 3);
        assert!(parse(&rect(2.0, 2.0, 20.0, 20.0, 4.0)).is_ok());
        assert!(parse(&rect(2.0, 2.0, 20.0, 20.0, 0.0)).is_ok());
    }

    fn bezier_point(p0: (f32, f32), c: &[f32; 6], t: f32) -> (f32, f32) {
        let mt = 1.0 - t;
        let x = mt * mt * mt * p0.0 + 3.0 * mt * mt * t * c[0] + 3.0 * mt * t * t * c[2] + t * t * t * c[4];
        let y = mt * mt * mt * p0.1 + 3.0 * mt * mt * t * c[1] + 3.0 * mt * t * t * c[3] + t * t * t * c[5];
        (x, y)
    }

    /// Every sampled point of the flattened arc must lie on the circle.
    fn assert_on_circle(start: (f32, f32), cubics: &[[f32; 6]], cx: f32, cy: f32, r: f32) {
        let mut p0 = start;
        for c in cubics {
            for i in 0..=8 {
                let (x, y) = bezier_point(p0, c, i as f32 / 8.0);
                let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
                assert!((d - r).abs() < r * 0.003 + 1e-3, "point ({x},{y}) is {d} from centre, want {r}");
            }
            p0 = (c[4], c[5]);
        }
    }

    #[test]
    fn quarter_arc_clockwise() {
        // (10,0) → (0,10) around the origin, sweep=1 (clockwise on screen), small arc.
        let c = arc_to_cubics(10.0, 0.0, 10.0, 10.0, 0.0, false, true, 0.0, 10.0);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0][4], c[0][5]), (0.0, 10.0));
        assert_on_circle((10.0, 0.0), &c, 0.0, 0.0, 10.0);
    }

    #[test]
    fn large_arc_goes_the_long_way() {
        // Same endpoints, large arc, sweep=0 → 270° around the origin.
        let c = arc_to_cubics(10.0, 0.0, 10.0, 10.0, 0.0, true, false, 0.0, 10.0);
        assert_eq!(c.len(), 3);
        assert_on_circle((10.0, 0.0), &c, 0.0, 0.0, 10.0);
    }

    #[test]
    fn half_circles_from_helper() {
        // The circle helper: two 180° arcs about (12,12) r=9.
        let segs = parse(&circle(12.0, 12.0, 9.0)).unwrap();
        let mut cur = (0.0, 0.0);
        for s in segs {
            match s {
                Seg::MoveTo(x, y) => cur = (x, y),
                Seg::ArcTo(rx, ry, rot, large, sweep, x, y) => {
                    let c = arc_to_cubics(cur.0, cur.1, rx, ry, rot, large, sweep, x, y);
                    assert_eq!(c.len(), 2);
                    assert_on_circle(cur, &c, 12.0, 12.0, 9.0);
                    cur = (x, y);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn radii_scale_up_and_degenerate_cases() {
        // Radius too small to span the endpoints: scaled to a half circle of r=5.
        let c = arc_to_cubics(0.0, 0.0, 1.0, 1.0, 0.0, false, true, 10.0, 0.0);
        assert_on_circle((0.0, 0.0), &c, 5.0, 0.0, 5.0);
        assert!(arc_to_cubics(1.0, 1.0, 5.0, 5.0, 0.0, false, true, 1.0, 1.0).is_empty());
        assert_eq!(arc_to_cubics(0.0, 0.0, 0.0, 5.0, 0.0, false, true, 4.0, 0.0).len(), 1);
    }

    #[test]
    fn errors() {
        assert!(parse("L1 1").is_ok() || parse("L1 1").is_err());
        assert!(parse("M1").is_err());
        assert!(parse("M1 1 X2").is_err());
    }
}
