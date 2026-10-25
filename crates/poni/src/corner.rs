//! Show over games, corner style (a test, `Settings::game_readout`): the
//! readout always on, as its own small window in the top-left corner of the
//! game's screen, where RivaTuner and NVIDIA's overlay sit, but outside the
//! game.
//!
//! Any window over a borderless game makes Windows compose the game's frames,
//! which costs about a frame of delay, unless the display hardware can show
//! the window on a plane of its own (a multiplane overlay). Windows only
//! offers that to content it can scan out as it is: a flip-model swap chain
//! shown through DirectComposition, with no window bitmap of its own. So this
//! window has none (`WS_EX_NOREDIRECTIONBITMAP`), draws only into a small
//! swap chain, and changes once a second. Graphics cards differ in what their
//! overlay planes take, so there are three styles to measure with PresentMon
//! (tools/overlay-check): see `GameReadout`.

use poni_core::settings::GameReadout;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{w, Interface, Result, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DeviceContext, ID2D1Factory1, ID2D1Image, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectComposition::{IDCompositionDevice, IDCompositionTarget, IDCompositionVisual};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_IGNORE, DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, SetLayeredWindowAttributes, SetWindowPos, ShowWindow, HTTRANSPARENT,
    HWND_TOPMOST, LWA_ALPHA, MA_NOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, WM_MOUSEACTIVATE,
    WM_NCHITTEST, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};

const CLASS: PCWSTR = w!("Poni.Corner");
static REGISTERED: AtomicBool = AtomicBool::new(false);

/// The window's size (DIPs) and its gap from the screen's corner.
const W: f32 = 236.0;
const H: f32 = 26.0;
const MARGIN: f32 = 12.0;
const TEXT_SIZE: f32 = 13.0;

/// The readout window. Dropping it destroys the window.
pub struct Corner {
    hwnd: HWND,
    style: GameReadout,
    gpu: CornerGpu,
    scale: f32,
    shown: bool,
    text: String,
}

struct CornerGpu {
    // Declaration order is drop order: the composition target first.
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    _dcomp: IDCompositionDevice,
    dc: ID2D1DeviceContext,
    swap: IDXGISwapChain1,
    format: IDWriteTextFormat,
}

unsafe extern "system" fn corner_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // Never in the way: clicks go to what's under it, and it never takes the focus.
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn register() -> Result<()> {
    if REGISTERED.load(Ordering::Relaxed) {
        return Ok(());
    }
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(corner_wndproc),
            hInstance: GetModuleHandleW(None)?.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(windows::core::Error::from_win32());
        }
    }
    REGISTERED.store(true, Ordering::Relaxed);
    Ok(())
}

fn opaque(style: GameReadout) -> bool {
    style == GameReadout::CornerOpaque
}

impl Corner {
    /// A hidden readout window in `style`, sized for a screen at `scale`.
    pub fn new(style: GameReadout, scale: f32) -> Result<Corner> {
        register()?;
        let scale = scale.max(0.5);
        let (w, h) = ((W * scale).round() as i32, (H * scale).round() as i32);
        let mut ex = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP;
        if style == GameReadout::CornerLayered {
            ex |= WS_EX_LAYERED | WS_EX_TRANSPARENT;
        }
        let hwnd = unsafe {
            CreateWindowExW(ex, CLASS, w!("Oyenza Studio readout"), WS_POPUP, 0, 0, w, h, None, None, GetModuleHandleW(None)?, None)?
        };
        if style == GameReadout::CornerLayered {
            // A layered window draws nothing of its own until it has attributes.
            unsafe {
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            }
        }
        let gpu = match unsafe { CornerGpu::new(hwnd, w as u32, h as u32, opaque(style)) } {
            Ok(g) => g,
            Err(e) => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                return Err(e);
            }
        };
        Ok(Corner { hwnd, style, gpu, scale, shown: false, text: String::new() })
    }

    pub fn style(&self) -> GameReadout {
        self.style
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Shows it in the top-left corner of `monitor` (physical pixels), on top.
    pub fn show(&mut self, monitor: RECT) {
        let m = (MARGIN * self.scale).round() as i32;
        let (w, h) = ((W * self.scale).round() as i32, (H * self.scale).round() as i32);
        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, monitor.left + m, monitor.top + m, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        self.shown = true;
    }

    pub fn hide(&mut self) {
        if self.shown {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.shown = false;
        }
    }

    pub fn shown(&self) -> bool {
        self.shown
    }

    /// Back on top after another window came forward.
    pub fn raise(&self) {
        if self.shown {
            unsafe {
                let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
    }

    /// Draws `text` and presents it (once a second at most: only when it changed).
    pub fn update(&mut self, text: &str) -> Result<()> {
        if text == self.text {
            return Ok(());
        }
        unsafe { self.gpu.draw(text, self.scale, opaque(self.style))? };
        self.text = text.to_string();
        Ok(())
    }
}

impl Drop for Corner {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

impl CornerGpu {
    unsafe fn new(hwnd: HWND, w: u32, h: u32, opaque: bool) -> Result<Self> {
        let (d3d, _ctx, _warp) = crate::gfx::create_d3d_device()?;
        let dxgi: IDXGIDevice = d3d.cast()?;
        let d2d: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dc = d2d.CreateDevice(&dxgi)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let factory: IDXGIFactory2 = dxgi.GetAdapter()?.GetParent()?;
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: w,
            Height: h,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            // An opaque plane is what the most overlay hardware accepts.
            AlphaMode: if opaque { DXGI_ALPHA_MODE_IGNORE } else { DXGI_ALPHA_MODE_PREMULTIPLIED },
            ..Default::default()
        };
        let swap = factory.CreateSwapChainForComposition(&d3d, &desc, None)?;
        let dcomp = crate::gfx::dcomp_create_device(&dxgi)?;
        let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
        let visual = dcomp.CreateVisual()?;
        visual.SetContent(&swap)?;
        target.SetRoot(&visual)?;
        dcomp.Commit()?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let format = dwrite.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            TEXT_SIZE,
            w!("en-us"),
        )?;
        format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        Ok(Self { _target: target, _visual: visual, _dcomp: dcomp, dc, swap, format })
    }

    unsafe fn draw(&mut self, text: &str, scale: f32, opaque: bool) -> Result<()> {
        let surface: IDXGISurface = self.swap.GetBuffer(0)?;
        let dpi = 96.0 * scale;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: if opaque { D2D1_ALPHA_MODE_IGNORE } else { D2D1_ALPHA_MODE_PREMULTIPLIED },
            },
            dpiX: dpi,
            dpiY: dpi,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: ManuallyDrop::new(None),
        };
        let bmp = self.dc.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
        self.dc.SetTarget(&bmp);
        self.dc.SetDpi(dpi, dpi);
        self.dc.BeginDraw();
        let black = |a: f32| D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a };
        if opaque {
            self.dc.Clear(Some(&black(1.0)));
        } else {
            self.dc.Clear(Some(&black(0.0)));
            let bg = self.dc.CreateSolidColorBrush(&black(0.62), None)?;
            let r = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: 0.0, top: 0.0, right: W, bottom: H }, radiusX: 7.0, radiusY: 7.0 };
            self.dc.FillRoundedRectangle(&r, &bg);
        }
        let ink = self.dc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.96 }, None)?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        let layout = D2D_RECT_F { left: 9.0, top: 0.0, right: W - 6.0, bottom: H };
        self.dc.DrawText(&wide, &self.format, &layout, &ink, D2D1_DRAW_TEXT_OPTIONS_NONE, Default::default());
        let end = self.dc.EndDraw(None, None);
        self.dc.SetTarget(None::<&ID2D1Image>);
        end?;
        self.swap.Present(0, DXGI_PRESENT(0)).ok()
    }
}

/// The self-test's check: each style's window can be made, drawn and shown
/// for a moment (on the GPU renderer: it needs DirectComposition).
pub fn probe(monitor: RECT, scale: f32) -> std::result::Result<String, String> {
    let mut done = Vec::new();
    for style in [GameReadout::Corner, GameReadout::CornerOpaque, GameReadout::CornerLayered] {
        let mut c = Corner::new(style, scale).map_err(|e| format!("{style:?}: {e}"))?;
        c.show(monitor);
        c.update("144 FPS \u{b7} GPU 97% 71\u{b0} \u{b7} CPU 38%").map_err(|e| format!("{style:?} drawing: {e}"))?;
        c.hide();
        done.push(format!("{style:?}"));
    }
    Ok(format!("drawn and shown: {}", done.join(", ")))
}
