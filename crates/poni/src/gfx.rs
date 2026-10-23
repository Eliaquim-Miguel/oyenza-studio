//! Rendering: Direct2D (software) into a 32-bit premultiplied DIB that is
//! pushed to a per-pixel-alpha layered window with `UpdateLayeredWindow`.
//!
//! Why a layered window instead of DirectComposition: transparent pixels are
//! click-through for free (so only the island itself catches the mouse), the
//! window can be resized and moved atomically every frame, and it renders
//! identically on machines without a GPU (CI runners, VMs). Frames are only
//! produced while something animates, so idle CPU stays at zero.
//!
//! All drawing coordinates are DIPs; the render target's DPI is set to the
//! monitor DPI so Direct2D does the scaling.

use crate::icons::{self, Icon};
use poni_core::geom::{Color, Point, Rect};
use poni_core::svgpath::Seg;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use windows::core::{w, Result, PCWSTR};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

// ---------------------------------------------------------------- images

/// Decoded image: premultiplied BGRA, top-down rows, `width * 4` stride.
pub struct ImageData {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for ImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImageData#{}({}x{})", self.id, self.width, self.height)
    }
}

pub type Image = Arc<ImageData>;

static NEXT_IMAGE_ID: AtomicU64 = AtomicU64::new(1);

impl ImageData {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Image {
        debug_assert_eq!(pixels.len(), (width * height * 4) as usize);
        Arc::new(ImageData { id: NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed), width, height, pixels })
    }

    /// A saturated, reasonably bright colour from the image (for accents),
    /// falling back to the plain average for greyscale art.
    pub fn accent_color(&self) -> Color {
        let (mut best, mut best_score) = (None, 0.0f32);
        let (mut sr, mut sg, mut sb, mut n) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let px = (self.width * self.height) as usize;
        let step = (px / 2048).max(1);
        let mut i = 0;
        while i < px {
            let o = i * 4;
            let a = self.pixels[o + 3] as f32 / 255.0;
            if a > 0.5 {
                let b = self.pixels[o] as f32 / 255.0 / a;
                let g = self.pixels[o + 1] as f32 / 255.0 / a;
                let r = self.pixels[o + 2] as f32 / 255.0 / a;
                sr += r;
                sg += g;
                sb += b;
                n += 1.0;
                let max = r.max(g).max(b);
                let min = r.min(g).min(b);
                let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
                let score = sat * (1.0 - (max - 0.75).abs());
                if score > best_score {
                    best_score = score;
                    best = Some(Color::rgba(r.min(1.0), g.min(1.0), b.min(1.0), 1.0));
                }
            }
            i += step;
        }
        match best {
            Some(c) if best_score > 0.25 => c.for_island_accent(),
            _ if n > 0.0 => Color::rgba(sr / n, sg / n, sb / n, 1.0).for_island_accent(),
            _ => Color::hex(0x0A84FF),
        }
    }
}

// ---------------------------------------------------------------- text

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
    Bold,
}

impl Weight {
    fn dwrite(self) -> DWRITE_FONT_WEIGHT {
        match self {
            Weight::Regular => DWRITE_FONT_WEIGHT_NORMAL,
            Weight::Medium => DWRITE_FONT_WEIGHT_MEDIUM,
            Weight::Semibold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
            Weight::Bold => DWRITE_FONT_WEIGHT_BOLD,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub weight: Weight,
    pub color: Color,
    pub align: Align,
    /// Fixed-width digits (timers, clocks) so numbers don't jiggle.
    pub tabular: bool,
    /// Wrap onto several lines instead of a single ellipsized line.
    pub wrap: bool,
    /// Vertically centre inside the rect (default) or top-align.
    pub vcenter: bool,
}

impl TextStyle {
    pub fn new(size: f32) -> Self {
        Self { size, weight: Weight::Regular, color: poni_core::geom::palette::TEXT, align: Align::Left, tabular: false, wrap: false, vcenter: true }
    }
    pub fn weight(mut self, w: Weight) -> Self {
        self.weight = w;
        self
    }
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }
    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }
    pub fn center(self) -> Self {
        self.align(Align::Center)
    }
    pub fn right(self) -> Self {
        self.align(Align::Right)
    }
    pub fn tabular(mut self) -> Self {
        self.tabular = true;
        self
    }
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }
    pub fn top(mut self) -> Self {
        self.vcenter = false;
        self
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn d2d_color(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.r, g: c.g, b: c.b, a: c.a.clamp(0.0, 1.0) }
}

fn rectf(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h }
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

// ---------------------------------------------------------------- Gfx

pub struct Gfx {
    pub d2d: ID2D1Factory,
    pub dwrite: IDWriteFactory,
    rt: ID2D1DCRenderTarget,
    mem_dc: HDC,
    dib: HBITMAP,
    old_bmp: HGDIOBJ,
    bits: *mut u8,
    cap_w: i32,
    cap_h: i32,
    /// Size in pixels of the frame being drawn / last presented.
    frame_w: i32,
    frame_h: i32,
    pub dpi: f32,
    brush: ID2D1SolidColorBrush,
    round_stroke: ID2D1StrokeStyle,
    family: Vec<u16>,
    formats: HashMap<(u32, Weight, bool), IDWriteTextFormat>,
    bitmaps: HashMap<u64, ID2D1Bitmap>,
    icon_geoms: HashMap<Icon, Vec<(ID2D1PathGeometry, bool)>>,
    /// Bitmaps used during the current frame (others are evicted at `end`).
    used_bitmaps: Vec<u64>,
}

impl Drop for Gfx {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem_dc, self.old_bmp);
            let _ = DeleteObject(self.dib);
            let _ = DeleteDC(self.mem_dc);
        }
    }
}

fn pick_font_family(dwrite: &IDWriteFactory) -> Vec<u16> {
    // Windows 11 ships "Segoe UI Variable"; Windows 10 has "Segoe UI".
    unsafe {
        let mut coll: Option<IDWriteFontCollection> = None;
        if dwrite.GetSystemFontCollection(&mut coll, false).is_ok() {
            if let Some(coll) = coll {
                for name in ["Segoe UI Variable Text", "Segoe UI Variable", "Segoe UI"] {
                    let w: Vec<u16> = wide(name).into_iter().chain(Some(0)).collect();
                    let mut index = 0u32;
                    let mut exists = BOOL(0);
                    if coll.FindFamilyName(PCWSTR(w.as_ptr()), &mut index, &mut exists).is_ok() && exists.as_bool() {
                        return w;
                    }
                }
            }
        }
    }
    wide("Segoe UI").into_iter().chain(Some(0)).collect()
}

impl Gfx {
    pub fn new(dpi: f32) -> Result<Self> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 0.0,
                dpiY: 0.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let rt = d2d.CreateDCRenderTarget(&props)?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            let brush = rt.CreateSolidColorBrush(&d2d_color(Color::white(1.0)), None)?;
            let round_stroke = d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )?;
            let mem_dc = CreateCompatibleDC(None);
            let family = pick_font_family(&dwrite);
            let mut g = Gfx {
                d2d,
                dwrite,
                rt,
                mem_dc,
                dib: HBITMAP::default(),
                old_bmp: HGDIOBJ::default(),
                bits: std::ptr::null_mut(),
                cap_w: 0,
                cap_h: 0,
                frame_w: 0,
                frame_h: 0,
                dpi,
                brush,
                round_stroke,
                family,
                formats: HashMap::new(),
                bitmaps: HashMap::new(),
                icon_geoms: HashMap::new(),
                used_bitmaps: Vec::new(),
            };
            g.ensure_capacity(512, 128)?;
            Ok(g)
        }
    }

    pub fn scale(&self) -> f32 {
        self.dpi / 96.0
    }

    pub fn set_dpi(&mut self, dpi: f32) {
        self.dpi = dpi;
    }

    /// Make sure the DIB can hold a `w`×`h` pixel frame.
    pub fn ensure_capacity(&mut self, w: i32, h: i32) -> Result<()> {
        if w <= self.cap_w && h <= self.cap_h && !self.bits.is_null() {
            return Ok(());
        }
        let nw = ((w.max(self.cap_w) + 63) / 64) * 64;
        let nh = ((h.max(self.cap_h) + 63) / 64) * 64;
        unsafe {
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: nw,
                    biHeight: -nh, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(self.mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let prev = SelectObject(self.mem_dc, dib);
            if self.dib.is_invalid() {
                self.old_bmp = prev;
            } else {
                let _ = DeleteObject(self.dib);
            }
            self.dib = dib;
            self.bits = bits as *mut u8;
            self.cap_w = nw;
            self.cap_h = nh;
        }
        Ok(())
    }

    /// Start a frame of `w`×`h` pixels. Returns a canvas in DIPs.
    pub fn begin(&mut self, w_px: i32, h_px: i32) -> Result<Canvas<'_>> {
        let w_px = w_px.max(1);
        let h_px = h_px.max(1);
        self.ensure_capacity(w_px, h_px)?;
        self.frame_w = w_px;
        self.frame_h = h_px;
        self.used_bitmaps.clear();
        unsafe {
            let rc = RECT { left: 0, top: 0, right: w_px, bottom: h_px };
            self.rt.BindDC(self.mem_dc, &rc)?;
            self.rt.SetDpi(self.dpi, self.dpi);
            self.rt.BeginDraw();
            self.rt.SetTransform(&Matrix3x2::identity());
            self.rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
        }
        Ok(Canvas { g: self, alpha: 1.0 })
    }

    /// Finish the frame. Evicts bitmaps that weren't used this frame once the
    /// cache grows large.
    pub fn end(&mut self) -> Result<()> {
        unsafe { self.rt.EndDraw(None, None)? };
        if self.bitmaps.len() > 48 {
            let used = std::mem::take(&mut self.used_bitmaps);
            self.bitmaps.retain(|k, _| used.contains(k));
        }
        Ok(())
    }

    /// Push the last frame to the layered window at (`x`, `y`) screen pixels.
    pub fn present(&self, hwnd: HWND, x: i32, y: i32) -> Result<()> {
        unsafe {
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            UpdateLayeredWindow(
                hwnd,
                None,
                Some(&POINT { x, y }),
                Some(&SIZE { cx: self.frame_w, cy: self.frame_h }),
                self.mem_dc,
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }

    pub fn frame_size(&self) -> (i32, i32) {
        (self.frame_w, self.frame_h)
    }

    /// Copy of the last frame's premultiplied BGRA pixels (for screenshots).
    pub fn frame_pixels(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity((self.frame_w * self.frame_h * 4) as usize);
        unsafe {
            for y in 0..self.frame_h {
                let row = self.bits.add((y * self.cap_w * 4) as usize);
                out.extend_from_slice(std::slice::from_raw_parts(row, (self.frame_w * 4) as usize));
            }
        }
        out
    }

    fn format(&mut self, size: f32, weight: Weight, wrap: bool) -> Result<IDWriteTextFormat> {
        let key = ((size * 100.0) as u32, weight, wrap);
        if let Some(f) = self.formats.get(&key) {
            return Ok(f.clone());
        }
        let f = unsafe {
            let f = self.dwrite.CreateTextFormat(
                PCWSTR(self.family.as_ptr()),
                None::<&IDWriteFontCollection>,
                weight.dwrite(),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("en-us"),
            )?;
            f.SetWordWrapping(if wrap { DWRITE_WORD_WRAPPING_WRAP } else { DWRITE_WORD_WRAPPING_NO_WRAP })?;
            if !wrap {
                let sign = self.dwrite.CreateEllipsisTrimmingSign(&f)?;
                let trimming = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                f.SetTrimming(&trimming, &sign)?;
            }
            f
        };
        self.formats.insert(key, f.clone());
        Ok(f)
    }

    fn layout(&mut self, s: &str, st: &TextStyle, w: f32, h: f32) -> Result<IDWriteTextLayout> {
        let fmt = self.format(st.size, st.weight, st.wrap)?;
        let text = wide(s);
        unsafe {
            let layout = self.dwrite.CreateTextLayout(&text, &fmt, w.max(1.0), h.max(1.0))?;
            layout.SetTextAlignment(match st.align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            })?;
            layout.SetParagraphAlignment(if st.vcenter { DWRITE_PARAGRAPH_ALIGNMENT_CENTER } else { DWRITE_PARAGRAPH_ALIGNMENT_NEAR })?;
            if st.tabular && !text.is_empty() {
                let typo = self.dwrite.CreateTypography()?;
                typo.AddFontFeature(DWRITE_FONT_FEATURE { nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, parameter: 1 })?;
                layout.SetTypography(&typo, DWRITE_TEXT_RANGE { startPosition: 0, length: text.len() as u32 })?;
            }
            Ok(layout)
        }
    }

    fn bitmap_for(&mut self, img: &Image) -> Result<ID2D1Bitmap> {
        self.used_bitmaps.push(img.id);
        if let Some(b) = self.bitmaps.get(&img.id) {
            return Ok(b.clone());
        }
        let bmp = unsafe {
            self.rt.CreateBitmap(
                D2D_SIZE_U { width: img.width, height: img.height },
                Some(img.pixels.as_ptr() as *const c_void),
                img.width * 4,
                &D2D1_BITMAP_PROPERTIES {
                    pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                    dpiX: 96.0,
                    dpiY: 96.0,
                },
            )?
        };
        self.bitmaps.insert(img.id, bmp.clone());
        Ok(bmp)
    }

    /// Build Direct2D geometry from parsed SVG segments.
    pub fn path_geometry(&self, segs: &[Seg], close_fill: bool) -> Result<ID2D1PathGeometry> {
        unsafe {
            let geo = self.d2d.CreatePathGeometry()?;
            let sink = geo.Open()?;
            sink.SetFillMode(D2D1_FILL_MODE_WINDING);
            let mut open = false;
            let mut cur = (0.0f32, 0.0f32);
            let begin = if close_fill { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW };
            for s in segs {
                match *s {
                    Seg::MoveTo(x, y) => {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                        }
                        sink.BeginFigure(pt(x, y), begin);
                        open = true;
                        cur = (x, y);
                    }
                    Seg::LineTo(x, y) => {
                        if !open {
                            sink.BeginFigure(pt(cur.0, cur.1), begin);
                            open = true;
                        }
                        sink.AddLine(pt(x, y));
                        cur = (x, y);
                    }
                    Seg::CubicTo(x1, y1, x2, y2, x, y) => {
                        if !open {
                            sink.BeginFigure(pt(cur.0, cur.1), begin);
                            open = true;
                        }
                        sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: pt(x1, y1), point2: pt(x2, y2), point3: pt(x, y) });
                        cur = (x, y);
                    }
                    Seg::QuadTo(x1, y1, x, y) => {
                        if !open {
                            sink.BeginFigure(pt(cur.0, cur.1), begin);
                            open = true;
                        }
                        sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT { point1: pt(x1, y1), point2: pt(x, y) });
                        cur = (x, y);
                    }
                    Seg::ArcTo(rx, ry, rot, large, sweep, x, y) => {
                        if !open {
                            sink.BeginFigure(pt(cur.0, cur.1), begin);
                            open = true;
                        }
                        sink.AddArc(&D2D1_ARC_SEGMENT {
                            point: pt(x, y),
                            size: D2D_SIZE_F { width: rx, height: ry },
                            rotationAngle: rot,
                            sweepDirection: if sweep { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
                            arcSize: if large { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                        });
                        cur = (x, y);
                    }
                    Seg::Close => {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            open = false;
                        }
                    }
                }
            }
            if open {
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            sink.Close()?;
            Ok(geo)
        }
    }

    fn icon_geometry(&mut self, icon: Icon) -> Vec<(ID2D1PathGeometry, bool)> {
        if let Some(g) = self.icon_geoms.get(&icon) {
            return g.clone();
        }
        let mut out = Vec::new();
        for part in icons::parts(icon) {
            match poni_core::svgpath::parse(part.d) {
                Ok(segs) => match self.path_geometry(&segs, part.fill) {
                    Ok(geo) => out.push((geo, part.fill)),
                    Err(e) => crate::pwarn!("icon {icon:?} geometry failed: {e}"),
                },
                Err(e) => crate::pwarn!("icon {icon:?} path parse failed: {}", e.0),
            }
        }
        self.icon_geoms.insert(icon, out.clone());
        out
    }
}

// ---------------------------------------------------------------- Canvas

/// Drawing API used by pages and activities. Coordinates are DIPs.
pub struct Canvas<'a> {
    g: &'a mut Gfx,
    /// Global opacity multiplier applied to everything drawn.
    alpha: f32,
}

impl<'a> Canvas<'a> {
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    pub fn set_alpha(&mut self, a: f32) {
        self.alpha = a.clamp(0.0, 1.0);
    }

    /// Run `f` with opacity multiplied by `a`.
    pub fn with_alpha<R>(&mut self, a: f32, f: impl FnOnce(&mut Canvas<'a>) -> R) -> R {
        let prev = self.alpha;
        self.alpha = (prev * a).clamp(0.0, 1.0);
        let r = f(self);
        self.alpha = prev;
        r
    }

    fn set_brush(&self, c: Color) {
        unsafe { self.g.brush.SetColor(&d2d_color(c.mul_alpha(self.alpha))) };
    }

    pub fn scale(&self) -> f32 {
        self.g.scale()
    }

    pub fn fill_rect(&mut self, r: Rect, c: Color) {
        self.set_brush(c);
        unsafe { self.g.rt.FillRectangle(&rectf(r), &self.g.brush) };
    }

    pub fn fill_rrect(&mut self, r: Rect, radius: f32, c: Color) {
        self.set_brush(c);
        let rr = D2D1_ROUNDED_RECT { rect: rectf(r), radiusX: radius, radiusY: radius };
        unsafe { self.g.rt.FillRoundedRectangle(&rr, &self.g.brush) };
    }

    pub fn stroke_rrect(&mut self, r: Rect, radius: f32, c: Color, width: f32) {
        self.set_brush(c);
        let rr = D2D1_ROUNDED_RECT { rect: rectf(r.inset(width / 2.0, width / 2.0)), radiusX: radius, radiusY: radius };
        unsafe { self.g.rt.DrawRoundedRectangle(&rr, &self.g.brush, width, &self.g.round_stroke) };
    }

    pub fn fill_circle(&mut self, c: Point, radius: f32, color: Color) {
        self.set_brush(color);
        let e = D2D1_ELLIPSE { point: pt(c.x, c.y), radiusX: radius, radiusY: radius };
        unsafe { self.g.rt.FillEllipse(&e, &self.g.brush) };
    }

    pub fn stroke_circle(&mut self, c: Point, radius: f32, color: Color, width: f32) {
        self.set_brush(color);
        let e = D2D1_ELLIPSE { point: pt(c.x, c.y), radiusX: radius, radiusY: radius };
        unsafe { self.g.rt.DrawEllipse(&e, &self.g.brush, width, &self.g.round_stroke) };
    }

    pub fn line(&mut self, a: Point, b: Point, c: Color, width: f32) {
        self.set_brush(c);
        unsafe { self.g.rt.DrawLine(pt(a.x, a.y), pt(b.x, b.y), &self.g.brush, width, &self.g.round_stroke) };
    }

    /// Arc clockwise from 12 o'clock: starts at `start` (0..1 of a turn) and
    /// spans `sweep` (0..1). Round caps.
    pub fn arc(&mut self, c: Point, radius: f32, start: f32, sweep: f32, color: Color, width: f32) {
        let sweep = sweep.clamp(0.0, 1.0);
        if sweep <= 0.0005 {
            return;
        }
        if sweep >= 0.9995 {
            self.stroke_circle(c, radius, color, width);
            return;
        }
        let ang = |f: f32| (f * std::f32::consts::TAU) - std::f32::consts::FRAC_PI_2;
        let (a0, a1) = (ang(start), ang(start + sweep));
        let p0 = (c.x + radius * a0.cos(), c.y + radius * a0.sin());
        let p1 = (c.x + radius * a1.cos(), c.y + radius * a1.sin());
        let segs = [Seg::MoveTo(p0.0, p0.1), Seg::ArcTo(radius, radius, 0.0, sweep > 0.5, true, p1.0, p1.1)];
        if let Ok(geo) = self.g.path_geometry(&segs, false) {
            self.set_brush(color);
            unsafe { self.g.rt.DrawGeometry(&geo, &self.g.brush, width, &self.g.round_stroke) };
        }
    }

    /// Progress ring: full track circle plus a progress arc on top.
    pub fn ring(&mut self, c: Point, radius: f32, width: f32, progress: f32, track: Color, fill: Color) {
        self.stroke_circle(c, radius, track, width);
        self.arc(c, radius, 0.0, progress, fill, width);
    }

    /// Fill and/or stroke arbitrary path segments.
    pub fn path(&mut self, segs: &[Seg], fill: Option<Color>, stroke: Option<(Color, f32)>) {
        if let Ok(geo) = self.g.path_geometry(segs, fill.is_some()) {
            unsafe {
                if let Some(f) = fill {
                    self.set_brush(f);
                    self.g.rt.FillGeometry(&geo, &self.g.brush, None);
                }
                if let Some((c, w)) = stroke {
                    self.set_brush(c);
                    self.g.rt.DrawGeometry(&geo, &self.g.brush, w, &self.g.round_stroke);
                }
            }
        }
    }

    /// The island silhouette: flat top flush with the screen edge, concave
    /// "shoulders" flaring into the top edge and rounded bottom corners.
    /// `body` is the main rectangle; shoulders extend `shoulder` DIPs outside it.
    pub fn notch(&mut self, body: Rect, radius: f32, shoulder: f32, fill: Color, border: Option<Color>) {
        let s = shoulder.max(0.0);
        let r = radius.min(body.h).min(body.w / 2.0).max(0.0);
        let (x0, x1, y1) = (body.x, body.right(), body.bottom());
        let mut segs = vec![Seg::MoveTo(x0 - s, 0.0)];
        if s > 0.0 {
            segs.push(Seg::ArcTo(s, s, 0.0, false, true, x0, s.min(body.h)));
        }
        segs.push(Seg::LineTo(x0, (y1 - r).max(0.0)));
        segs.push(Seg::ArcTo(r, r, 0.0, false, false, x0 + r, y1));
        segs.push(Seg::LineTo(x1 - r, y1));
        segs.push(Seg::ArcTo(r, r, 0.0, false, false, x1, (y1 - r).max(0.0)));
        segs.push(Seg::LineTo(x1, s.min(body.h)));
        if s > 0.0 {
            segs.push(Seg::ArcTo(s, s, 0.0, false, true, x1 + s, 0.0));
        }
        segs.push(Seg::Close);
        self.path(&segs, Some(fill), border.map(|b| (b, 1.0)));
    }

    /// Draw text in `r`. Single-line text is ellipsized; see [`TextStyle`].
    pub fn text(&mut self, s: &str, r: Rect, st: &TextStyle) {
        if s.is_empty() || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        if let Ok(layout) = self.g.layout(s, st, r.w, r.h) {
            self.set_brush(st.color);
            unsafe { self.g.rt.DrawTextLayout(pt(r.x, r.y), &layout, &self.g.brush, D2D1_DRAW_TEXT_OPTIONS_NONE) };
        }
    }

    /// Measure text: (width, height) in DIPs, laid out within `max_w`.
    pub fn measure(&mut self, s: &str, st: &TextStyle, max_w: f32) -> (f32, f32) {
        let mut st = *st;
        st.align = Align::Left;
        match self.g.layout(s, &st, max_w.max(1.0), 10_000.0) {
            Ok(layout) => unsafe {
                let mut m = DWRITE_TEXT_METRICS::default();
                if layout.GetMetrics(&mut m).is_ok() {
                    (m.widthIncludingTrailingWhitespace, m.height)
                } else {
                    (0.0, st.size * 1.3)
                }
            },
            Err(_) => (0.0, st.size * 1.3),
        }
    }

    /// Draw an image cover-fitted into `r` with rounded corners.
    pub fn image(&mut self, img: &Image, r: Rect, radius: f32) {
        if img.width == 0 || img.height == 0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let Ok(bmp) = self.g.bitmap_for(img) else { return };
        unsafe {
            let props = D2D1_BITMAP_BRUSH_PROPERTIES {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            };
            let Ok(brush) = self.g.rt.CreateBitmapBrush(&bmp, Some(&props), None) else { return };
            // Bitmaps are created at 96 DPI so their DIP size equals pixel size.
            let (iw, ih) = (img.width as f32, img.height as f32);
            let s = (r.w / iw).max(r.h / ih);
            let (dw, dh) = (iw * s, ih * s);
            let ox = r.x + (r.w - dw) / 2.0;
            let oy = r.y + (r.h - dh) / 2.0;
            brush.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: ox, M32: oy });
            brush.SetOpacity(self.alpha);
            let rr = D2D1_ROUNDED_RECT { rect: rectf(r), radiusX: radius, radiusY: radius };
            self.g.rt.FillRoundedRectangle(&rr, &brush);
        }
    }

    /// Draw a vector icon (24×24 design grid) scaled into `r`.
    pub fn icon(&mut self, icon: Icon, r: Rect, color: Color) {
        let geoms = self.g.icon_geometry(icon);
        if geoms.is_empty() {
            return;
        }
        let s = r.w.min(r.h) / 24.0;
        let ox = r.x + (r.w - 24.0 * s) / 2.0;
        let oy = r.y + (r.h - 24.0 * s) / 2.0;
        self.set_brush(color);
        unsafe {
            let mut prev = Matrix3x2::identity();
            self.g.rt.GetTransform(&mut prev);
            let m = Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: ox, M32: oy };
            self.g.rt.SetTransform(&(m * prev));
            for (geo, fill) in &geoms {
                if *fill {
                    self.g.rt.FillGeometry(geo, &self.g.brush, None);
                } else {
                    self.g.rt.DrawGeometry(geo, &self.g.brush, icons::STROKE, &self.g.round_stroke);
                }
            }
            self.g.rt.SetTransform(&prev);
        }
    }

    /// Clip subsequent drawing to `r` until [`Canvas::unclip`].
    pub fn clip(&mut self, r: Rect) {
        unsafe { self.g.rt.PushAxisAlignedClip(&rectf(r), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    }

    pub fn unclip(&mut self) {
        unsafe { self.g.rt.PopAxisAlignedClip() };
    }

    /// Translate subsequent drawing by (dx, dy) until [`Canvas::reset_transform`].
    pub fn translate(&mut self, dx: f32, dy: f32) {
        unsafe {
            let mut prev = Matrix3x2::identity();
            self.g.rt.GetTransform(&mut prev);
            self.g.rt.SetTransform(&(Matrix3x2::translation(dx, dy) * prev));
        }
    }

    pub fn reset_transform(&mut self) {
        unsafe { self.g.rt.SetTransform(&Matrix3x2::identity()) };
    }
}
