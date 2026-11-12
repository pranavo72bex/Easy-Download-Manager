// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! macOS: through AppKit, on the window's `NSWindow`.

use super::*;

/// The window's `NSWindow`, through the system's handle.
pub(crate) fn ns_window(window: &Window) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    // The system's handle, not GPUI's own of the same name.
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: `ns_view` is the live content view of `window`, which this
    // borrow of the window keeps open; only its window is read, on the main
    // thread, where this runs.
    let view: &objc2_app_kit::NSView = unsafe { appkit.ns_view.cast().as_ref() };
    view.window()
}

/// Puts `window` away without closing it (`-[NSWindow orderOut:]`), as the
/// egui window hides into the tray.
pub fn hide(window: &Window) {
    if let Some(ns_window) = ns_window(window) {
        ns_window.orderOut(None);
    }
}

/// Puts `window`'s top-left corner at (`x`, `y`), in the points of the
/// screen whose top-left is (0, 0), as `ui.windows` reports where windows
/// stand.
pub fn move_to(window: &Window, x: f32, y: f32) -> Result<(), String> {
    let ns_window = ns_window(window).ok_or("the window has no system window")?;
    let main = objc2::MainThreadMarker::new().ok_or("not on the main thread")?;
    // AppKit counts from the bottom of the first screen, upwards.
    let screens = objc2_app_kit::NSScreen::screens(main);
    let first = screens.firstObject().ok_or("there is no screen")?;
    let height = first.frame().size.height;
    ns_window.setFrameTopLeftPoint(objc2_foundation::NSPoint::new(
        f64::from(x),
        height - f64::from(y),
    ));
    Ok(())
}

/// Pictures `window` as it stands on screen, its frame without its shadow,
/// into the PNG at `path` (the system's `screencapture`).
pub fn picture(window: &Window, path: &std::path::Path) -> Result<(), String> {
    let ns_window = ns_window(window).ok_or("the window has no system window")?;
    let number = ns_window.windowNumber();
    folder_for(path)?;
    let done = std::process::Command::new("/usr/sbin/screencapture")
        .arg("-x")
        .arg("-o")
        .arg(format!("-l{number}"))
        .arg(path)
        .status()
        .map_err(|e| format!("the window could not be pictured: {e}"))?;
    if done.success() && path.is_file() {
        Ok(())
    } else {
        Err("the window could not be pictured".into())
    }
}

/// A hidden window can always be put away here.
pub fn nowhere_to_hide(_: &Window) -> bool {
    false
}

/// Putting the window in front is enough to bring it back from the tray.
pub fn show(_: &Window) {}
