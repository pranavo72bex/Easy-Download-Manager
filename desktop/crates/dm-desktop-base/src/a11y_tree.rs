// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a screen reader is given of each of dm's windows on macOS, read in dm itself: each
//! window's accessibility children, walked through the NSAccessibility protocol, which is where
//! VoiceOver's requests end up (AccessKit answers them for the window's widgets, egui's or GPUI's). Read from inside, as
//! another program would need the Accessibility permission to read it from outside (gaps row
//! 160). Behind the control protocol's `ui.accessibility`.

use serde_json::Value;

/// The most elements read, and the deepest, so a large list cannot make the answer endless.
#[cfg(target_os = "macos")]
const MOST_ELEMENTS: usize = 3000;
#[cfg(target_os = "macos")]
const DEEPEST: usize = 40;

/// Each visible window's title and the elements a screen reader is given in it: role, label and
/// title, with their depth. AppKit asks this on the main thread; elsewhere it is refused.
#[cfg(target_os = "macos")]
pub fn windows() -> Result<Value, String> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, MainThreadMarker};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{NSArray, NSString};

    let mtm = MainThreadMarker::new().ok_or("the windows are read on the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: `windows`, `title`, `isVisible` and the NSAccessibility getters are AppKit's
    // documented methods, asked on the main thread of objects AppKit hands out; each optional
    // getter is asked only of an object that says it answers it.
    unsafe {
        let answers = |object: &AnyObject, selector: objc2::runtime::Sel| -> bool {
            msg_send![object, respondsToSelector: selector]
        };
        let text = |object: &AnyObject, selector: objc2::runtime::Sel| -> String {
            if !answers(object, selector) {
                return String::new();
            }
            let value: Option<Retained<AnyObject>> = match selector.name().to_bytes() {
                b"accessibilityRole" => msg_send![object, accessibilityRole],
                b"accessibilityLabel" => msg_send![object, accessibilityLabel],
                b"accessibilityTitle" => msg_send![object, accessibilityTitle],
                b"title" => msg_send![object, title],
                _ => None,
            };
            value
                .and_then(|v| v.downcast::<NSString>().ok())
                .map(|s| s.to_string())
                .unwrap_or_default()
        };
        let windows: Retained<NSArray<AnyObject>> = msg_send![&*app, windows];
        let mut out = Vec::new();
        for window in windows.iter() {
            let visible: bool = msg_send![&*window, isVisible];
            if !visible {
                continue;
            }
            let mut elements = Vec::new();
            let mut stack: Vec<(Retained<AnyObject>, usize)> = vec![(window.clone(), 0)];
            while let Some((element, depth)) = stack.pop() {
                if elements.len() >= MOST_ELEMENTS {
                    break;
                }
                if depth > 0 {
                    elements.push(serde_json::json!({
                        "depth": depth,
                        "role": text(&element, objc2::sel!(accessibilityRole)),
                        "label": text(&element, objc2::sel!(accessibilityLabel)),
                        "title": text(&element, objc2::sel!(accessibilityTitle)),
                    }));
                }
                if depth >= DEEPEST || !answers(&element, objc2::sel!(accessibilityChildren)) {
                    continue;
                }
                let children: Option<Retained<NSArray<AnyObject>>> =
                    msg_send![&*element, accessibilityChildren];
                // Pushed last first, so they are read in order.
                for child in children
                    .iter()
                    .flat_map(|c| c.iter())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                {
                    stack.push((child, depth + 1));
                }
            }
            out.push(serde_json::json!({
                "title": text(&window, objc2::sel!(title)),
                "elements": elements,
            }));
        }
        Ok(serde_json::json!({ "windows": out }))
    }
}

/// Read on macOS only: Linux's is read over AT-SPI (`scripts/linux-gui-check.sh`) and Windows'
/// over UI Automation (`scripts/windows-a11y-check.py`), from outside, as their screen readers do.
#[cfg(not(target_os = "macos"))]
pub fn windows() -> Result<Value, String> {
    Err("read on macOS only: Linux is read over AT-SPI and Windows over UI Automation".into())
}
