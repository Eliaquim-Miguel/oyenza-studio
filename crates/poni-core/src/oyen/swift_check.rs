//! The cross-check with the Swift port: frames OyenKit wrote must come out of
//! this port the same. Every mood at several moments and both eye sizes, the
//! Fokus moods with their inputs, and a set of whole SVG drawings. Numbers (the
//! head's transform, the fold, stroke widths, opacities) must agree within
//! 1e-9, and every path's SVG text exactly. On Linux the numbers agree to the
//! bit; another libm (Windows CI) may round a last digit differently.
//!
//! The fixture was written by a small Swift program built with OyenKit's own
//! sources (oyenza-bar commit 213172663). The program is at the fixture's end,
//! with how to run it again when OyenKit changes; its header gives the records.

use super::svg::{self, Canvas, Style};
use super::*;

const FIXTURE: &str = include_str!("../../tests/fixtures/oyen_swift_frames.txt");

#[derive(Default)]
struct Report {
    frames: usize,
    drawings: usize,
    numbers: usize,
    same_bits: usize,
    max_diff: f64,
    problems: Vec<String>,
}

impl Report {
    fn number(&mut self, what: &str, ours: f64, theirs: &str) {
        let theirs: f64 = theirs.parse().unwrap_or(f64::NAN);
        let diff = (ours - theirs).abs();
        self.numbers += 1;
        if ours.to_bits() == theirs.to_bits() {
            self.same_bits += 1;
        }
        self.max_diff = self.max_diff.max(diff);
        if diff.is_nan() || diff > 1e-9 {
            self.problems
                .push(format!("{what}: {ours:?}, Swift {theirs:?}"));
        }
    }

    fn text(&mut self, what: &str, ours: &str, theirs: &str) {
        if ours != theirs {
            self.problems
                .push(format!("{what}:\n  ours  {ours}\n  Swift {theirs}"));
        }
    }

    /// `fields`: stroke (or "-"), paint, place, opacity, path, clip (or "-").
    fn shape(&mut self, what: &str, ours: &Shape, fields: &[&str]) {
        match (ours.stroke, fields[0]) {
            (None, "-") => {}
            (Some(width), theirs) if theirs != "-" => {
                self.number(&format!("{what} stroke"), width, theirs)
            }
            (ours, theirs) => self
                .problems
                .push(format!("{what} stroke: {ours:?}, Swift {theirs}")),
        }
        self.text(&format!("{what} paint"), paint_name(ours.paint), fields[1]);
        self.text(&format!("{what} place"), place_name(ours.place), fields[2]);
        self.number(&format!("{what} opacity"), ours.opacity, fields[3]);
        self.text(&format!("{what} path"), &ours.path.to_svg(), fields[4]);
        let clip = ours
            .clip
            .as_ref()
            .map_or_else(|| "-".to_string(), Path::to_svg);
        self.text(&format!("{what} clip"), &clip, fields[5]);
    }

    /// `header`: mood, t, eye scale, pencil, breath; `records`: the frame's lines.
    fn frame(&mut self, header: &[&str], records: &[Vec<&str>]) {
        self.frames += 1;
        let mood = Mood::from_name(header[0]).expect("a mood the port knows");
        let [t, eye_scale, pencil, breath] =
            [1, 2, 3, 4].map(|i| header[i].parse::<f64>().expect("a number"));
        let ours = frame(mood, t, Input { pencil, breath }, eye_scale);
        let what = format!(
            "{} at {t} (eyes x{eye_scale}, pencil {pencil}, breath {breath})",
            header[0]
        );
        let mut eyes = ours.eyes.iter();
        let mut extras = ours.extras.iter();
        for fields in records {
            match fields[0] {
                "head" => {
                    let h = ours.head;
                    for (i, (name, value)) in [
                        ("a", h.a),
                        ("b", h.b),
                        ("c", h.c),
                        ("d", h.d),
                        ("tx", h.tx),
                        ("ty", h.ty),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        self.number(&format!("{what} head.{name}"), value, fields[i + 1]);
                    }
                }
                "fold" => self.number(&format!("{what} fold"), ours.fold, fields[1]),
                "tip" => {
                    self.text(
                        &format!("{what} tip faces up"),
                        if ours.tip.faces_up { "1" } else { "0" },
                        fields[1],
                    );
                    self.text(
                        &format!("{what} tip behind head"),
                        if ours.tip.behind_head { "1" } else { "0" },
                        fields[2],
                    );
                    let path = if &ours.tip.path == fold_path() {
                        "mark".to_string()
                    } else {
                        ours.tip.path.to_svg()
                    };
                    self.text(&format!("{what} tip"), &path, fields[3]);
                }
                "eye" | "extra" => {
                    let next = if fields[0] == "eye" {
                        eyes.next()
                    } else {
                        extras.next()
                    };
                    match next {
                        Some(shape) => {
                            self.shape(&format!("{what} {}", fields[0]), shape, &fields[1..])
                        }
                        None => self
                            .problems
                            .push(format!("{what}: Swift has another {}", fields[0])),
                    }
                }
                other => panic!("unknown fixture record {other:?}"),
            }
        }
        if eyes.next().is_some() || extras.next().is_some() {
            self.problems
                .push(format!("{what}: we draw more shapes than Swift"));
        }
    }

    /// `fields`: kind, mood, t, eye scale, the Swift port's text.
    fn drawing(&mut self, fields: &[&str]) {
        self.drawings += 1;
        let mood = Mood::from_name(fields[1]).expect("a mood the port knows");
        let [t, eye_scale] = [2, 3].map(|i| fields[i].parse::<f64>().expect("a number"));
        let ours = drawn(fields[0], &frame(mood, t, Input::default(), eye_scale));
        self.text(
            &format!("{} {} at {t} (eyes x{eye_scale})", fields[0], fields[1]),
            &ours,
            fields[4],
        );
    }
}

/// A frame drawn the way the fixture's SVG `kind` says (the review page's two
/// bar cuts, and the app comparison's whole documents).
fn drawn(kind: &str, f: &Frame) -> String {
    let bar = Canvas::menu_bar(18.0);
    match kind {
        "colour" => svg::body(
            f,
            Style::Colour {
                ink: "currentColor",
            },
            bar,
            "@@",
            Some("oyen"),
        ),
        "template" => svg::body(
            f,
            Style::Template {
                color: "currentColor",
                gap: 4.6,
            },
            bar,
            "@@",
            Some("oyen"),
        ),
        "colour-document" => svg::document(
            f,
            Style::Colour {
                ink: &svg::css(INK),
            },
            Canvas::face(256.0),
            "oyen",
        ),
        "template-document" => svg::document(
            f,
            Style::Template {
                color: "#000",
                gap: 2.6,
            },
            bar,
            "t",
        ),
        other => panic!("unknown drawing kind {other:?}"),
    }
}

fn paint_name(paint: Paint) -> &'static str {
    match paint {
        Paint::Ink => "ink",
        Paint::Pink => "pink",
        Paint::Green => "green",
        Paint::White => "white",
    }
}

fn place_name(place: Place) -> &'static str {
    match place {
        Place::Eye => "eye",
        Place::Highlight => "highlight",
        Place::OnHead => "on_head",
        Place::Above => "above",
        Place::OverEar => "over_ear",
    }
}

#[test]
fn frames_match_the_swift_port() {
    let mut report = Report::default();
    let mut header: Option<Vec<&str>> = None;
    let mut records: Vec<Vec<&str>> = Vec::new();
    for line in FIXTURE
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[0] {
            "frame" | "svg" | "defs" => {
                if let Some(header) = header.take() {
                    report.frame(&header, &records);
                    records.clear();
                }
                match fields[0] {
                    "frame" => header = Some(fields[1..].to_vec()),
                    "svg" => report.drawing(&fields[1..]),
                    _ => report.text("shared outlines", &svg::shared_defs(fields[1]), fields[2]),
                }
            }
            _ => records.push(fields),
        }
    }
    if let Some(header) = header {
        report.frame(&header, &records);
    }

    println!(
        "{} frames and {} drawings compared; {} of {} numbers the same to the bit, largest difference {:e}",
        report.frames, report.drawings, report.same_bits, report.numbers, report.max_diff
    );
    assert!(
        report.frames >= 200 && report.drawings >= 20,
        "the fixture lost its frames"
    );
    let first: Vec<&str> = report
        .problems
        .iter()
        .take(12)
        .map(String::as_str)
        .collect();
    assert!(
        report.problems.is_empty(),
        "{} differences from the Swift port:\n{}",
        report.problems.len(),
        first.join("\n")
    );
}
