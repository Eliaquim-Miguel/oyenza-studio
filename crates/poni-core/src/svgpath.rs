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

    #[test]
    fn errors() {
        assert!(parse("L1 1").is_ok() || parse("L1 1").is_err());
        assert!(parse("M1").is_err());
        assert!(parse("M1 1 X2").is_err());
    }
}
