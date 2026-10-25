//! Full-screen apps on the Mac: the island steps off them, as on Windows
//! (Settings › In full-screen apps, or the same submenu in the menu bar).
//!
//! macOS does the hiding. With Hide picked, the island's panel no longer
//! joins full-screen Spaces (`window::over_fullscreen`), so when a video, a
//! game or a slideshow goes full screen the window server leaves the island
//! on the desktop's Space. The app still needs to know, to close the island,
//! keep the prayer sound quiet and stop drawing: the panel's occlusion
//! changes when its Space goes out of view, and NSWorkspace says when a
//! Space changes. Both are notifications; nothing polls.
//!
//! Stay on top keeps the island on full-screen Spaces too, as before. A game
//! that covers the screen with an ordinary window rather than a Space of its
//! own isn't noticed: Windows watches windows move for that, and macOS shows
//! window moves only to apps with the Accessibility permission.

use crate::events::{AppEvent, EventSink};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSWindow, NSWindowCollectionBehavior,
    NSWindowDidChangeOcclusionStateNotification, NSWindowStyleMask, NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSTimer};
use std::ptr::NonNull;
use std::sync::Mutex;

static SINK: Mutex<Option<EventSink>> = Mutex::new(None);

/// Tell the app whether the island is out of view on its screen (the app
/// ignores a repeat of what it knows).
fn report() {
    let off = crate::window::off_its_space();
    if let Ok(g) = SINK.lock() {
        if let Some(s) = g.as_ref() {
            s.send(AppEvent::Fullscreen(off));
        }
    }
}

define_class!(
    /// Hears the island's panel go out of view or back, and Spaces change (main thread).
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PoniFullScreenObserver"]
    struct Observer;

    unsafe impl NSObjectProtocol for Observer {}

    impl Observer {
        #[unsafe(method(changed:))]
        fn changed(&self, _n: &NSNotification) {
            report();
        }
    }
);

/// Start listening, for the app's life (after the panel exists).
pub fn watch(mtm: MainThreadMarker, sink: EventSink) {
    if let Ok(mut g) = SINK.lock() {
        *g = Some(sink);
    }
    let observer: Retained<Observer> = unsafe { msg_send![Observer::alloc(mtm), init] };
    let obs: &AnyObject = observer.as_ref();
    let panel = crate::window::panel();
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            obs,
            sel!(changed:),
            Some(NSWindowDidChangeOcclusionStateNotification),
            panel.as_deref().map(|p| p.as_ref()),
        );
        NSWorkspace::sharedWorkspace().notificationCenter().addObserver_selector_name_object(
            obs,
            sel!(changed:),
            Some(NSWorkspaceActiveSpaceDidChangeNotification),
            None,
        );
    }
    std::mem::forget(observer);
    report();
}

/// The smoke test's full-screen app (`poni --full-screen-window <seconds>`):
/// a plain window that goes full screen into a Space of its own, as a video
/// or a game does, and quits after the given seconds.
pub fn test_window(secs: f64) -> i32 {
    let Some(mtm) = MainThreadMarker::new() else { return 2 };
    let app = NSApplication::sharedApplication(mtm);
    // Full screen is for apps with a Dock icon (the island has none).
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let rect = NSRect::new(NSPoint::new(160.0, 160.0), NSSize::new(640.0, 400.0));
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Resizable;
    let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), rect, style, NSBackingStoreType::Buffered, false) };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setCollectionBehavior(NSWindowCollectionBehavior::FullScreenPrimary);
    window.setBackgroundColor(Some(&NSColor::systemTealColor()));
    window.makeKeyAndOrderFront(None);
    app.activate();
    let full = RcBlock::new(move |_t: NonNull<NSTimer>| window.toggleFullScreen(None));
    let quit = RcBlock::new(|_t: NonNull<NSTimer>| std::process::exit(0));
    unsafe {
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.5, false, &full);
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(secs.max(1.0), false, &quit);
    }
    app.run();
    0
}
