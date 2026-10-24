//! Plain geometry for Oyen's face, in the Oyenza mark's own units: the head is
//! 104 wide and centred on (0, 0), y grows downwards. f64 throughout, and every
//! expression keeps the Swift port's order of operations, so frames come out
//! the same to the bit (`to_radians()`, `powi()` and the like would not).

use std::f64::consts::PI;
use std::fmt::{self, Write};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// The point `k` of the way from here to `other`.
    pub fn lerp(self, other: Point, k: f64) -> Point {
        Point::new(
            self.x + (other.x - self.x) * k,
            self.y + (other.y - self.y) * k,
        )
    }
}

/// A 2D affine transform. A point maps to `(a·x + c·y + tx, b·x + d·y + ty)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    pub const fn translation(x: f64, y: f64) -> Affine {
        Affine {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: x,
            ty: y,
        }
    }

    pub const fn scale(x: f64, y: f64) -> Affine {
        Affine {
            a: x,
            b: 0.0,
            c: 0.0,
            d: y,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// A turn in degrees, clockwise on screen (y grows downwards), the way SVG's `rotate()` reads.
    pub fn rotation(degrees: f64) -> Affine {
        // Not `to_radians()`, which multiplies by a rounded pi / 180 and lands a bit elsewhere.
        let r = degrees * PI / 180.0;
        Affine {
            a: r.cos(),
            b: r.sin(),
            c: -r.sin(),
            d: r.cos(),
            tx: 0.0,
            ty: 0.0,
        }
    }

    pub fn rotation_about(degrees: f64, p: Point) -> Affine {
        Affine::translation(-p.x, -p.y)
            .then(Affine::rotation(degrees))
            .then(Affine::translation(p.x, p.y))
    }

    /// This transform first, then `next`.
    pub fn then(self, next: Affine) -> Affine {
        Affine {
            a: next.a * self.a + next.c * self.b,
            b: next.b * self.a + next.d * self.b,
            c: next.a * self.c + next.c * self.d,
            d: next.b * self.c + next.d * self.d,
            tx: next.a * self.tx + next.c * self.ty + next.tx,
            ty: next.b * self.tx + next.d * self.ty + next.ty,
        }
    }

    pub fn apply(self, p: Point) -> Point {
        Point::new(
            self.a * p.x + self.c * p.y + self.tx,
            self.b * p.x + self.d * p.y + self.ty,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Element {
    Move(Point),
    Line(Point),
    /// Control point, end point.
    Quad(Point, Point),
    /// First control point, second control point, end point.
    Cubic(Point, Point, Point),
    Close,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub elements: Vec<Element>,
}

/// How far along the tangent a cubic's control points sit for a quarter ellipse.
const KAPPA: f64 = 0.552_284_749_8;

impl Path {
    pub fn new(elements: Vec<Element>) -> Path {
        Path { elements }
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    pub fn polygon(points: &[Point]) -> Path {
        let mut path = Path::polyline(points);
        if !path.is_empty() {
            path.elements.push(Element::Close);
        }
        path
    }

    pub fn polyline(points: &[Point]) -> Path {
        let Some((&first, rest)) = points.split_first() else {
            return Path::default();
        };
        let mut elements = Vec::with_capacity(points.len() + 1);
        elements.push(Element::Move(first));
        elements.extend(rest.iter().map(|&p| Element::Line(p)));
        Path { elements }
    }

    pub fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Path {
        let kx = rx * KAPPA;
        let ky = ry * KAPPA;
        let p = Point::new;
        Path::new(vec![
            Element::Move(p(cx + rx, cy)),
            Element::Cubic(p(cx + rx, cy + ky), p(cx + kx, cy + ry), p(cx, cy + ry)),
            Element::Cubic(p(cx - kx, cy + ry), p(cx - rx, cy + ky), p(cx - rx, cy)),
            Element::Cubic(p(cx - rx, cy - ky), p(cx - kx, cy - ry), p(cx, cy - ry)),
            Element::Cubic(p(cx + kx, cy - ry), p(cx + rx, cy - ky), p(cx + rx, cy)),
            Element::Close,
        ])
    }

    /// The lower half of an ellipse with a flat top, from its left end to its right end.
    pub fn lower_half_ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Path {
        let kx = rx * KAPPA;
        let ky = ry * KAPPA;
        let p = Point::new;
        Path::new(vec![
            Element::Move(p(cx - rx, cy)),
            Element::Cubic(p(cx - rx, cy + ky), p(cx - kx, cy + ry), p(cx, cy + ry)),
            Element::Cubic(p(cx + kx, cy + ry), p(cx + rx, cy + ky), p(cx + rx, cy)),
            Element::Close,
        ])
    }

    pub fn applying(&self, m: Affine) -> Path {
        let elements = self
            .elements
            .iter()
            .map(|e| match *e {
                Element::Move(p) => Element::Move(m.apply(p)),
                Element::Line(p) => Element::Line(m.apply(p)),
                Element::Quad(c, p) => Element::Quad(m.apply(c), m.apply(p)),
                Element::Cubic(c1, c2, p) => Element::Cubic(m.apply(c1), m.apply(c2), m.apply(p)),
                Element::Close => Element::Close,
            })
            .collect();
        Path { elements }
    }

    /// Every point the path names, control points included: a box around them
    /// always contains the path.
    pub fn points(&self) -> Vec<Point> {
        let mut out = Vec::new();
        for e in &self.elements {
            match *e {
                Element::Move(p) | Element::Line(p) => out.push(p),
                Element::Quad(c, p) => out.extend([c, p]),
                Element::Cubic(c1, c2, p) => out.extend([c1, c2, p]),
                Element::Close => {}
            }
        }
        out
    }

    /// Reads absolute `M`, `L`, `Q`, `C` and `Z` path data, which is all the mark's files use.
    pub fn from_svg(data: &str) -> Result<Path, PathError> {
        let mut elements = Vec::new();
        let mut scanner = Scanner {
            chars: data.chars().collect(),
            index: 0,
        };
        let mut command: Option<char> = None;
        while let Some(next) = scanner.peek_significant() {
            if next.is_alphabetic() {
                scanner.index += 1;
                command = Some(next);
                if next == 'Z' || next == 'z' {
                    elements.push(Element::Close);
                    command = None;
                }
                continue;
            }
            let Some(active) = command else {
                return Err(PathError::Malformed("number before any command".into()));
            };
            match active {
                'M' => {
                    elements.push(Element::Move(scanner.point()?));
                    command = Some('L'); // further pairs after a move are lines
                }
                'L' => elements.push(Element::Line(scanner.point()?)),
                'Q' => {
                    let control = scanner.point()?;
                    elements.push(Element::Quad(control, scanner.point()?));
                }
                'C' => {
                    let c1 = scanner.point()?;
                    let c2 = scanner.point()?;
                    elements.push(Element::Cubic(c1, c2, scanner.point()?));
                }
                other => return Err(PathError::UnsupportedCommand(other)),
            }
        }
        Ok(Path { elements })
    }

    /// Path data with two decimals, the precision the app's own component writes.
    pub fn to_svg(&self) -> String {
        let mut out = String::new();
        for e in &self.elements {
            // Writing to a String cannot fail.
            let _ = match *e {
                Element::Move(p) => write!(out, "M{}", Pair(p)),
                Element::Line(p) => write!(out, "L{}", Pair(p)),
                Element::Quad(c, p) => write!(out, "Q{} {}", Pair(c), Pair(p)),
                Element::Cubic(c1, c2, p) => write!(out, "C{} {} {}", Pair(c1), Pair(c2), Pair(p)),
                Element::Close => write!(out, "Z"),
            };
        }
        out
    }
}

struct Pair(Point);

impl fmt::Display for Pair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", number(self.0.x), number(self.0.y))
    }
}

/// A number with at most two decimals and no trailing zeros ("-0" never appears).
pub fn number(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == rounded.round() {
        return format!("{}", rounded as i64);
    }
    format!("{rounded:.2}").trim_end_matches('0').to_string()
}

#[derive(Clone, Debug, PartialEq)]
pub enum PathError {
    UnsupportedCommand(char),
    Malformed(String),
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::UnsupportedCommand(c) => write!(f, "unsupported path command '{c}'"),
            PathError::Malformed(why) => write!(f, "malformed path data: {why}"),
        }
    }
}

impl std::error::Error for PathError {}

struct Scanner {
    chars: Vec<char>,
    index: usize,
}

impl Scanner {
    fn peek_significant(&mut self) -> Option<char> {
        while let Some(&c) = self.chars.get(self.index) {
            if matches!(c, ' ' | ',' | '\n' | '\t') {
                self.index += 1;
            } else {
                return Some(c);
            }
        }
        None
    }

    fn point(&mut self) -> Result<Point, PathError> {
        let x = self.number()?;
        let y = self.number()?;
        Ok(Point::new(x, y))
    }

    fn number(&mut self) -> Result<f64, PathError> {
        if self.peek_significant().is_none() {
            return Err(PathError::Malformed("number expected at the end".into()));
        }
        let start = self.index;
        let mut seen_digit = false;
        while let Some(&c) = self.chars.get(self.index) {
            let after_exponent =
                self.index > start && matches!(self.chars[self.index - 1], 'e' | 'E');
            if c.is_ascii_digit() {
                seen_digit = true;
            } else if matches!(c, '.' | 'e' | 'E') {
                // part of the number
            } else if matches!(c, '-' | '+') && (self.index == start || after_exponent) {
                // a leading sign, or the exponent's
            } else {
                break;
            }
            self.index += 1;
        }
        let text: String = self.chars[start..self.index].iter().collect();
        match text.parse::<f64>() {
            Ok(value) if seen_digit => Ok(value),
            _ => Err(PathError::Malformed(format!("not a number: {text}"))),
        }
    }
}

/// `x` unless `y` is smaller. Ties and NaNs go the Swift port's way (`f64::min`
/// may take either zero of a -0/0 pair), so the sign of a zero, and with it
/// every "-0.00000" the SVG writer prints, lands the same way.
pub(crate) fn min(x: f64, y: f64) -> f64 {
    if y < x {
        y
    } else {
        x
    }
}

/// `y` unless `x` is larger; see [`min`].
pub(crate) fn max(x: f64, y: f64) -> f64 {
    if y >= x {
        y
    } else {
        x
    }
}

/// A polygon with rounded corners, as points: each corner with a radius
/// becomes a short arc of `segments` steps. The construction the mark's own
/// tooling uses, so the folded ear's tip matches the logo. One radius per point.
pub fn rounded_polygon(points: &[Point], radii: &[f64], segments: usize) -> Vec<Point> {
    let mut out = Vec::new();
    let n = points.len();
    for i in 0..n {
        let p = points[i];
        let a = points[(i + n - 1) % n];
        let b = points[(i + 1) % n];
        let radius = radii[i];
        if radius == 0.0 {
            out.push(p);
            continue;
        }
        let mut ux = a.x - p.x;
        let mut uy = a.y - p.y;
        let lu = ux.hypot(uy);
        ux /= lu;
        uy /= lu;
        let mut vx = b.x - p.x;
        let mut vy = b.y - p.y;
        let lv = vx.hypot(vy);
        vx /= lv;
        vy /= lv;
        let angle = min(1.0, max(-1.0, ux * vx + uy * vy)).acos();
        let t = min(radius / (angle / 2.0).tan(), 0.48 * min(lu, lv));
        let rr = t * (angle / 2.0).tan();
        let mut bx = ux + vx;
        let mut by = uy + vy;
        let lb = bx.hypot(by);
        bx /= lb;
        by /= lb;
        let dc = rr / (angle / 2.0).sin();
        let cx = p.x + bx * dc;
        let cy = p.y + by * dc;
        let p1 = Point::new(p.x + ux * t, p.y + uy * t);
        let p2 = Point::new(p.x + vx * t, p.y + vy * t);
        let a1 = (p1.y - cy).atan2(p1.x - cx);
        let mut da = (p2.y - cy).atan2(p2.x - cx) - a1;
        while da > PI {
            da -= 2.0 * PI;
        }
        while da < -PI {
            da += 2.0 * PI;
        }
        for k in 0..=segments {
            let step = a1 + da * k as f64 / segments as f64;
            out.push(Point::new(cx + rr * step.cos(), cy + rr * step.sin()));
        }
    }
    out
}

/// The first `length` units of a polyline.
pub fn trim(points: &[Point], length: f64) -> Vec<Point> {
    let first = match points.first() {
        Some(&first) if length > 0.0 => first,
        _ => return Vec::new(),
    };
    let mut out = vec![first];
    let mut left = length;
    for pair in points.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let segment = (to.x - from.x).hypot(to.y - from.y);
        if segment <= left {
            out.push(to);
            left -= segment;
        } else {
            out.push(from.lerp(to, left / segment));
            return out;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_data_survives_a_round_trip() {
        let data = "M25.65 -58.33L25.65 -58.33Q28.39 -61.57 32.53 -60.63Q52 -9.16 52 3.81e-06Z";
        let path = Path::from_svg(data).unwrap();
        assert_eq!(path.elements.len(), 5);
        assert_eq!(
            path.to_svg(),
            "M25.65 -58.33L25.65 -58.33Q28.39 -61.57 32.53 -60.63Q52 -9.16 52 0Z"
        );
        // Written with two decimals, so reading it back gives the same text, not the same doubles.
        assert_eq!(
            Path::from_svg(&path.to_svg()).unwrap().to_svg(),
            path.to_svg()
        );
    }

    #[test]
    fn unknown_path_commands_are_refused() {
        assert_eq!(
            Path::from_svg("M0 0A5 5 0 0 0 10 10"),
            Err(PathError::UnsupportedCommand('A'))
        );
        assert_eq!(
            Path::from_svg("m0 0l5 5"),
            Err(PathError::UnsupportedCommand('m'))
        );
    }

    #[test]
    fn malformed_path_data_is_refused() {
        assert!(matches!(
            Path::from_svg("0 0L5 5"),
            Err(PathError::Malformed(_))
        ));
        assert!(matches!(Path::from_svg("M0"), Err(PathError::Malformed(_))));
        assert!(matches!(
            Path::from_svg("M1.2.3 4"),
            Err(PathError::Malformed(_))
        ));
        assert!(matches!(
            Path::from_svg("M0 0Z 1 1"),
            Err(PathError::Malformed(_))
        ));
    }

    #[test]
    fn path_numbers_pack_and_spread() {
        // Commas, tabs and a sign that starts the next number all separate.
        let path = Path::from_svg("M1,2\tL-3-4.5 C1e1 2E-1 +3 4 5 6").unwrap();
        assert_eq!(
            path.elements,
            vec![
                Element::Move(Point::new(1.0, 2.0)),
                Element::Line(Point::new(-3.0, -4.5)),
                Element::Cubic(
                    Point::new(10.0, 0.2),
                    Point::new(3.0, 4.0),
                    Point::new(5.0, 6.0)
                ),
            ]
        );
        // Pairs after a move are lines.
        assert_eq!(
            Path::from_svg("M0 0 1 1").unwrap().elements[1],
            Element::Line(Point::new(1.0, 1.0))
        );
    }

    #[test]
    fn numbers_have_two_decimals_and_never_minus_zero() {
        assert_eq!(number(0.0), "0");
        assert_eq!(number(-0.0), "0");
        assert_eq!(number(-0.004), "0");
        assert_eq!(number(3.81e-6), "0");
        assert_eq!(number(1.5), "1.5");
        assert_eq!(number(-12.345), "-12.35");
        assert_eq!(number(0.1 + 0.2), "0.3");
        assert_eq!(number(99.999), "100");
        assert_eq!(number(-0.05), "-0.05");
    }

    #[test]
    fn trimming_a_line_stops_part_of_the_way() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ];
        assert!(trim(&points, 0.0).is_empty());
        assert!(trim(&[], 5.0).is_empty());
        assert_eq!(
            trim(&points, 15.0),
            vec![
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 5.0)
            ]
        );
        assert_eq!(trim(&points, 99.0), points.to_vec());
    }

    #[test]
    fn transforms_compose_in_reading_order() {
        let m = Affine::translation(0.0, -10.0)
            .then(Affine::scale(2.0, 2.0))
            .then(Affine::translation(0.0, 10.0));
        assert_eq!(m.apply(Point::new(1.0, 10.0)), Point::new(2.0, 10.0));
        let turned = Affine::rotation(90.0).apply(Point::new(1.0, 0.0));
        assert!(turned.x.abs() < 1e-9);
        assert!((turned.y - 1.0).abs() < 1e-9);
        let about = Affine::rotation_about(180.0, Point::new(5.0, 5.0)).apply(Point::new(6.0, 5.0));
        assert!((about.x - 4.0).abs() < 1e-9 && (about.y - 5.0).abs() < 1e-9);
    }

    #[test]
    fn shapes_name_their_points() {
        let e = Path::ellipse(0.0, 0.0, 2.0, 1.0);
        assert_eq!(e.points().len(), 13);
        let xs: Vec<f64> = e.points().iter().map(|p| p.x).collect();
        assert_eq!(xs.iter().cloned().fold(f64::MIN, f64::max), 2.0);
        assert_eq!(Path::polygon(&[]), Path::default());
        assert_eq!(
            Path::polygon(&[Point::new(1.0, 1.0)]).elements.last(),
            Some(&Element::Close)
        );
        assert_eq!(
            Path::polyline(&[Point::new(1.0, 1.0), Point::new(2.0, 2.0)]).to_svg(),
            "M1 1L2 2"
        );
        assert_eq!(
            Path::lower_half_ellipse(0.0, 0.0, 1.0, 1.0).to_svg(),
            "M-1 0C-1 0.55 -0.55 1 0 1C0.55 1 1 0.55 1 0Z"
        );
    }

    #[test]
    fn min_and_max_keep_the_swift_ports_zero() {
        assert!(max(0.0, -0.0).is_sign_negative());
        assert!(min(1.0, -0.0).is_sign_negative());
        assert!(min(-0.0, 0.0).is_sign_negative());
        assert!(max(-0.0, 0.0).is_sign_positive());
        assert_eq!(min(2.0, 1.0), 1.0);
        assert_eq!(max(2.0, 1.0), 2.0);
    }

    #[test]
    fn rounded_corners_stay_inside_the_polygon() {
        let square = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let round = rounded_polygon(&square, &[2.0, 0.0, 2.0, 0.0], 4);
        // Two rounded corners of five points each, two sharp ones.
        assert_eq!(round.len(), 12);
        assert!(round
            .iter()
            .all(|p| (-1e-9..=10.0 + 1e-9).contains(&p.x) && (-1e-9..=10.0 + 1e-9).contains(&p.y)));
        // The first corner's arc runs from one edge to the other, 2 units in.
        let near = |p: Point, x: f64, y: f64| (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9;
        assert!(near(round[0], 0.0, 2.0) && near(round[4], 2.0, 0.0));
        assert_eq!(round[5], Point::new(10.0, 0.0));
    }
}
