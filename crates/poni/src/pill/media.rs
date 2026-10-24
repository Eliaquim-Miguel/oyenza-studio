//! Music, collapsed (`Activity::Media`): the album art on the left, the
//! line being sung in the middle ([`super::lyric`]) or, when there is
//! none, the song ("Title · artist"), the equalizer in the album's colour
//! on the right, and a glow of that colour along the bottom edge (ref3).
//! The pill is as wide as its text (spec 6.5), so it grows and shrinks a
//! little with the lyrics while the text stays centred.
//!
//! A new track's peek (`st.ui.peek_until`): text always shows here, so the
//! peek picks which text. The title holds the middle for the peek even
//! while a line is being sung, then the line takes over: a track change is
//! the moment people look up to see what's playing, and the lines have the
//! rest of the song. Most songs open with a few bars before anyone sings,
//! so the peek rarely holds a line back.
//!
//! Paused, the art and the text dim and the bars sink low and still; the
//! lingering paused pill comes and goes as in Classic (`activities`).

use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::island::Geom;
use crate::state::AppState;
use crate::ui::Ui;
use poni_core::geom::{clamp01, lerp, palette, Color, Rect};
use std::cell::Cell;

/// Every collapsed pill is this tall, a capsule (spec 6.1).
const H: f32 = 32.0;
/// The pill is its text plus this much: the art and the equalizer with
/// their gaps, the same on both sides so the text centres (spec 6.5).
const SIDES: f32 = 88.0;
const MIN_W: f32 = 240.0;
const MAX_W: f32 = 520.0;
/// Widths go in steps of this much, so lines a few DIPs apart don't each
/// morph the pill.
const W_STEP: f32 = 8.0;
/// The art: 20 DIPs, 6 in from the end (spec 6.4).
const ART: f32 = 20.0;
const ART_INSET: f32 = 6.0;
const ART_R: f32 = 6.0;
/// Text is cut this far from the art and the bars while the pill morphs.
const TEXT_CLEAR: f32 = 4.0;
/// The equalizer: 4 bars 3 wide and 2 apart, the last one 13 in from the
/// end, 4 to 14 tall (spec 6.4).
const BARS: usize = 4;
const BAR_W: f32 = 3.0;
const BAR_GAP: f32 = 2.0;
const EQ_RIGHT: f32 = 13.0;
const EQ_W: f32 = BARS as f32 * BAR_W + (BARS - 1) as f32 * BAR_GAP;
const BAR_LOW: f32 = 4.0;
const BAR_HIGH: f32 = 14.0;
/// Where the bars hold (of their range) while music plays on battery
/// saver: they stand still there rather than spend a frame a refresh.
const BARS_STILL: [f32; BARS] = [0.45, 0.85, 0.3, 0.65];
/// The glow: the bottom 15 DIPs, its alpha rising to 0.55 as t² (spec 6.5, 6.9).
const GLOW_BAND: f32 = 15.0;
const GLOW_ALPHA: f32 = 0.55;
/// Play and pause ease over this long: the glow, the bars, the dimming.
const PLAY_FADE: f64 = 0.2;
/// Paused, the art and the text are this opaque.
const PAUSED_ART: f32 = 0.55;
const PAUSED_TEXT: f32 = 0.65;
/// The title and the sung line cross-fade over this long when one takes
/// over from the other (the peek ending, the first line after an intro).
const HANDOFF: f64 = 0.15;

thread_local! {
    /// The pill width the text asked for on the last frame. `geom` has no
    /// canvas to measure with, so `draw` measures and keeps it here, and
    /// asks for one more frame when it changes (the pill follows a frame late).
    static FIT_W: Cell<f32> = const { Cell::new(MIN_W) };
    /// Whether music played on the last frame drawn, and since when (mono
    /// seconds), for the play/pause ease. It starts "paused since ever", so
    /// a pill that first shows paused doesn't fade.
    static PLAYING: Cell<(bool, f64)> = const { Cell::new((false, f64::NEG_INFINITY)) };
    /// Whether the middle showed the sung line (else the song) on the last
    /// frame drawn, and since when, for the hand-off between the two.
    static SUNG: Cell<(bool, f64)> = const { Cell::new((false, f64::NEG_INFINITY)) };
}

pub fn geom(_st: &AppState) -> Option<Geom> {
    Some(Geom::new(FIT_W.get(), H, H / 2.0))
}

pub fn draw(ui: &mut Ui, st: &mut AppState, body: Rect) -> bool {
    let now = st.now;
    let playing = st.media.as_ref().is_some_and(|m| m.is_playing());
    let level = play_level(playing, now);
    if level > 0.0 && level < 1.0 {
        ui.request_anim();
    }
    let cy = body.center().y;
    let accent = st.media_accent();

    // The glow lies under everything else.
    glow(ui, body, accent, level);
    let art = Rect::new(body.x + ART_INSET, cy - ART / 2.0, ART, ART);
    draw_art(ui, st, art, lerp(PAUSED_ART, 1.0, level));
    // The bars keep moving while they sink at a pause; on battery saver they stand.
    let moving = (playing || level > 0.0) && !st.power.saver;
    let eq_x = body.right() - EQ_RIGHT - EQ_W;
    equalizer(ui, eq_x, cy, accent, level, moving, now);

    // The text centres on the pill in a box as wide as the widest pill's
    // text, so its layout doesn't change with the pill's width on each
    // frame of a morph, cut where the art and the bars are.
    let area = Rect::new(body.center().x - (MAX_W - SIDES) / 2.0, cy - H / 2.0, MAX_W - SIDES, H);
    let x0 = art.right() + TEXT_CLEAR;
    ui.c.clip(Rect::new(x0, body.y, (eq_x - TEXT_CLEAR - x0).max(0.0), body.h));
    let want = draw_text(ui, st, area, lerp(PAUSED_TEXT, 1.0, level), now);
    ui.c.unclip();
    if want != FIT_W.get() {
        FIT_W.set(want);
        ui.request_repaint();
    }
    true
}

/// The bars move at the equalizer's ambient rate (near 60 fps, as in
/// Classic) while music plays, and not on battery saver: there they stand
/// still and the pill draws only when its text changes.
pub fn is_live(st: &AppState) -> Option<bool> {
    let playing = st.media.as_ref().is_some_and(|m| m.is_playing());
    Some(playing && !st.power.saver)
}

/// The next line and the end of the peek (the line takes over from the
/// title), whichever comes first.
pub fn next_wake(st: &AppState) -> Option<f64> {
    let peek = (st.ui.peek_until > st.now).then_some(st.ui.peek_until);
    match (super::lyric::next_wake(st), peek) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// How playing the pill looks, 0 (paused) to 1 (playing), easing over
/// [`PLAY_FADE`] after each change.
fn play_level(playing: bool, now: f64) -> f32 {
    let (was, since) = PLAYING.get();
    let since = if was == playing {
        since
    } else {
        PLAYING.set((playing, now));
        now
    };
    let k = smooth(clamp01(((now - since) / PLAY_FADE) as f32));
    if playing {
        k
    } else {
        1.0 - k
    }
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// The width the pill needs for text `w` wide.
fn fit(w: f32) -> f32 {
    (((w + SIDES) / W_STEP).ceil() * W_STEP).clamp(MIN_W, MAX_W)
}

/// The sung line as the lyric lane draws it (spec 6.5), to size the pill.
fn line_style() -> TextStyle {
    TextStyle::body().weight(Weight::Medium)
}

/// Run `f` with everything it draws `a` times as opaque.
fn faded<R>(ui: &mut Ui, a: f32, f: impl FnOnce(&mut Ui) -> R) -> R {
    let prev = ui.c.alpha();
    ui.c.set_alpha(prev * a);
    let r = f(ui);
    ui.c.set_alpha(prev);
    r
}

/// The middle: the line being sung, or the song. The two cross-fade when
/// one takes over from the other. Returns the pill width the text needs.
fn draw_text(ui: &mut Ui, st: &AppState, area: Rect, alpha: f32, now: f64) -> f32 {
    // The title holds the middle while a new track peeks.
    let peeking = now < st.ui.peek_until;
    let (was_sung, since) = SUNG.get();
    let k = smooth(clamp01(((now - since) / HANDOFF) as f32));
    let line_a = if was_sung { k } else { 0.0 };
    let sung = !peeking && faded(ui, alpha * line_a, |ui| super::lyric::draw(ui, st, area));
    let since = if sung == was_sung {
        since
    } else {
        SUNG.set((sung, now));
        now
    };
    let k = smooth(clamp01(((now - since) / HANDOFF) as f32));
    if k < 1.0 {
        ui.request_anim();
    }
    if !sung {
        // A line that just ended can't be drawn fading out (the lyric lane
        // has let go of it), so the title fades in on its own.
        return fit(draw_song(ui, st, area, alpha * k));
    }
    if k < 1.0 {
        draw_song(ui, st, area, alpha * (1.0 - k));
    }
    match super::lyric::current(st) {
        Some((line, _)) => fit(ui.c.measure(&line, &line_style(), 10_000.0).0),
        // The last line is still fading out: it keeps its width until it's gone.
        None => FIT_W.get(),
    }
}

/// "Title · artist" centred in `area`: the title semibold, the artist dim,
/// and the artist only when there is room for more than an ellipsis of it.
/// Returns how wide all of it is.
fn draw_song(ui: &mut Ui, st: &AppState, area: Rect, alpha: f32) -> f32 {
    let Some(m) = st.media.as_ref() else { return 0.0 };
    let (title, artist) = (m.title.trim(), m.artist.trim());
    // A player that names no song: its artist, or at least the player.
    let (main, rest) = match (title.is_empty(), artist.is_empty()) {
        (false, _) => (title, artist),
        (true, false) => (artist, ""),
        (true, true) => (m.app_name.trim(), ""),
    };
    if main.is_empty() {
        return 0.0;
    }
    let main_st = TextStyle::body().weight(Weight::Semibold);
    let rest_st = TextStyle::body().color(palette::TEXT_DIM);
    let rest = if rest.is_empty() { String::new() } else { format!(" \u{b7} {rest}") };
    let mw = ui.c.measure(main, &main_st, 10_000.0).0;
    let rw = if rest.is_empty() { 0.0 } else { ui.c.measure(&rest, &rest_st, 10_000.0).0 };
    if alpha > 0.002 {
        ui.c.with_alpha(alpha, |c| {
            if mw >= area.w {
                c.text(main, area, &main_st.center());
                return;
            }
            let room = area.w - mw;
            let shown = if rw > 0.0 && room >= 32.0 { rw.min(room) } else { 0.0 };
            let x = area.center().x - (mw + shown) / 2.0;
            c.text(main, Rect::new(x, area.y, mw + 2.0, area.h), &main_st);
            if shown > 0.0 {
                // A whole artist gets 2 DIPs to spare; a cut one ellipsizes at the edge.
                let w = if shown < rw { shown } else { rw + 2.0 };
                c.text(&rest, Rect::new(x + mw, area.y, w, area.h), &rest_st);
            }
        });
    }
    mw + rw
}

/// The album art, or a note on a soft tile when the player sends none.
fn draw_art(ui: &mut Ui, st: &AppState, r: Rect, alpha: f32) {
    let art = st.media.as_ref().and_then(|m| m.art.as_ref());
    ui.c.with_alpha(alpha, |c| match art {
        Some(img) => c.image(img, r, ART_R),
        None => {
            c.fill_rrect(r, ART_R, palette::SURFACE_HOVER);
            c.icon(Icon::Music, r.inset(4.0, 4.0), Color::white(0.85));
        }
    });
}

/// Four bars in the album's colour from `x`: rising and falling while
/// music plays, all low and still while it's paused (`level` eases from
/// one to the other).
fn equalizer(ui: &mut Ui, x: f32, cy: f32, color: Color, level: f32, moving: bool, t: f64) {
    let s = ui.c.scale().max(0.1);
    for (i, still) in BARS_STILL.iter().enumerate() {
        let k = if moving { bounce(i, t) } else { *still };
        let h = BAR_LOW + (BAR_HIGH - BAR_LOW) * k * level;
        // Whole pixels across: 3 DIP bars stay crisp at 100 %.
        let bx = ((x + i as f32 * (BAR_W + BAR_GAP)) * s).round() / s;
        ui.c.fill_rrect(Rect::new(bx, cy - h / 2.0, BAR_W, h), BAR_W / 2.0, color);
    }
}

/// How high bar `i` stands at `t` (0..1 of its range): two waves a bar at
/// rates that never line up, so the bars don't fall into one rhythm.
fn bounce(i: usize, t: f64) -> f32 {
    const SLOW: [(f64, f64); BARS] = [(8.2, 0.0), (10.6, 1.9), (7.4, 3.7), (9.5, 5.3)];
    const FAST: [(f64, f64); BARS] = [(13.3, 2.4), (15.8, 0.7), (12.1, 4.6), (14.6, 1.3)];
    let ((a, p), (b, q)) = (SLOW[i], FAST[i]);
    (0.5 + 0.3 * (a * t + p).sin() + 0.2 * (b * t + q).sin()).clamp(0.0, 1.0) as f32
}

/// The album's colour rising along the bottom edge (ref3), inside the
/// outline. `fill_path_fade` takes two stops, so the spec's t² curve is two
/// linear fades stacked: one over the band and one over its lower half.
/// Halfway down the first gives a quarter of the peak alone, as t² does,
/// and together they reach the peak at the edge.
fn glow(ui: &mut Ui, body: Rect, color: Color, level: f32) {
    let a = GLOW_ALPHA * level;
    if a <= 0.005 {
        return;
    }
    let outline = super::shape::segments(body, body.h / 2.0);
    let (y0, y1) = (body.bottom() - GLOW_BAND, body.bottom());
    let a1 = a / 2.0;
    let a2 = a1 / (1.0 - a1);
    // Only the band rasterizes.
    ui.c.clip(Rect::new(body.x, y0, body.w, GLOW_BAND));
    ui.c.fill_path_fade(&outline, color, y0, y1, 0.0, a1);
    ui.c.fill_path_fade(&outline, color, (y0 + y1) / 2.0, y1, 0.0, a2);
    ui.c.unclip();
}
