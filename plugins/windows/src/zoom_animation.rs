//! Live-resizing zoom animation for macOS windows.
//!
//! AppKit's `-[NSWindow zoom:]` (the green button, Window ▸ Zoom, and Tauri's
//! double-click-to-maximize on drag regions) animates the frame with a
//! blocking `setFrame:display:animate:` loop. The main thread doesn't return
//! to its run loop until the animation ends, so WKWebView can't commit the
//! frames its web process renders at the new sizes. For the whole animation
//! the window shows the old page pinned to the top-left corner, then snaps to
//! the final layout.
//!
//! We add `windowShouldZoom:toFrame:` to tao's window delegate. It tells AppKit
//! not to run its own animation and starts a non-blocking `NSViewAnimation` to
//! the same target frame instead. The run loop keeps turning between steps, so
//! the page lays out at every size on the way. AppKit still records the
//! pre-zoom frame, so zooming back returns to it and `isZoomed` stays right.

use std::cell::RefCell;
use std::ffi::{CStr, CString};

use objc2::encode::Encode;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{AllocAnyThread, ffi, sel};
use objc2_app_kit::{
    NSAnimationBlockingMode, NSAnimationCurve, NSViewAnimation, NSViewAnimationEndFrameKey,
    NSViewAnimationTargetKey, NSWindow, NSWorkspace,
};
use objc2_foundation::{NSArray, NSDictionary, NSRect, NSValue};

/// The `NSWindowDelegate` class tao registers for every window it creates.
const TAO_DELEGATE_CLASS: &CStr = c"TaoWindowDelegate";

type ShouldZoomFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut NSWindow, NSRect) -> Bool;

thread_local! {
    /// The zoom animation in flight for each window, so a second zoom can stop
    /// the first instead of fighting it. A running `NSAnimation` keeps itself
    /// alive, so weak references let finished ones (and their window) go.
    static ANIMATIONS: RefCell<Vec<(usize, Weak<NSViewAnimation>)>> = const { RefCell::new(Vec::new()) };
}

/// Adds the live zoom animation to every tao window. Call after a window has
/// been built, so tao has registered its delegate class. Safe to call again.
pub(crate) fn install() {
    let Some(class) = AnyClass::get(TAO_DELEGATE_CLASS) else {
        tracing::warn!("zoom_animation: tao window delegate class not found");
        return;
    };
    let selector = sel!(windowShouldZoom:toFrame:);
    if class.instance_method(selector).is_some() {
        return;
    }

    let types = CString::new(format!(
        "{}{}{}{}{}",
        Bool::ENCODING,
        <*mut AnyObject>::ENCODING,
        Sel::ENCODING,
        <*mut AnyObject>::ENCODING,
        NSRect::ENCODING,
    ))
    .expect("type encoding has no NUL bytes");
    let imp = unsafe { std::mem::transmute::<ShouldZoomFn, Imp>(window_should_zoom) };
    let added = unsafe {
        ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            selector,
            imp,
            types.as_ptr(),
        )
    };
    if !added.as_bool() {
        tracing::warn!("zoom_animation: could not add windowShouldZoom:toFrame:");
    }
}

unsafe extern "C-unwind" fn window_should_zoom(
    _this: *mut AnyObject,
    _cmd: Sel,
    window: *mut NSWindow,
    to_frame: NSRect,
) -> Bool {
    let Some(window) = (unsafe { window.as_ref() }) else {
        return Bool::YES;
    };
    // NO tells AppKit we're animating the zoom ourselves.
    Bool::new(!start_live_zoom(window, to_frame))
}

/// Starts a run-loop-friendly animation to `to_frame`. Returns false when
/// AppKit should zoom natively instead: hidden or minimized windows (e.g. the
/// saved maximized state being restored before the window is shown) and when
/// Reduce Motion is on.
fn start_live_zoom(window: &NSWindow, to_frame: NSRect) -> bool {
    if !window.isVisible()
        || window.isMiniaturized()
        || NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    {
        return false;
    }

    let key = window as *const NSWindow as usize;
    ANIMATIONS.with_borrow_mut(|animations| {
        animations.retain(|(window_key, animation)| {
            let Some(animation) = animation.load() else {
                return false;
            };
            if *window_key == key {
                animation.stopAnimation();
                return false;
            }
            true
        });
    });

    let end_frame = unsafe { NSValue::valueWithRect(to_frame) };
    let target: &AnyObject = window.as_ref();
    let end_frame: &AnyObject = end_frame.as_ref();
    let description = NSDictionary::from_slices(
        &[unsafe { NSViewAnimationTargetKey }, unsafe {
            NSViewAnimationEndFrameKey
        }],
        &[target, end_frame],
    );
    let animation: Retained<NSViewAnimation> = unsafe {
        NSViewAnimation::initWithViewAnimations(
            NSViewAnimation::alloc(),
            &NSArray::from_retained_slice(&[description]),
        )
    };
    animation.setDuration(window.animationResizeTime(to_frame));
    animation.setAnimationCurve(NSAnimationCurve::EaseInOut);
    unsafe { animation.setAnimationBlockingMode(NSAnimationBlockingMode::Nonblocking) };
    animation.startAnimation();

    ANIMATIONS
        .with_borrow_mut(|animations| animations.push((key, Weak::from_retained(&animation))));
    true
}
