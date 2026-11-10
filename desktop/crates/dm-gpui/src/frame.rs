// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The main window comes back where and as large as it was left, zoomed if
//! it was, as the egui window's does (eframe keeps its frame) and as a
//! Mac's windows do. The frame is kept in `window.json` in dm's home, so a
//! run on a home of its own (`DM_HOME`: a test, a check) keeps its own and
//! starts as a fresh install does. A frame no longer on any display (one
//! unplugged since) is not used: the window opens in the middle.

use std::path::Path;

use gpui::{point, px, size, Bounds, Pixels, Size, WindowBounds};

/// The file, in dm's home.
const FILE: &str = "window.json";

/// The frame kept at the last quit, if there is one that reads.
pub fn kept(home: &Path) -> Option<WindowBounds> {
    let text = std::fs::read_to_string(home.join(FILE)).ok()?;
    read(&text)
}

/// A frame as `window.json` says it.
fn read(text: &str) -> Option<WindowBounds> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let number = |key: &str| {
        value
            .get(key)?
            .as_f64()
            .filter(|n| n.is_finite())
            .map(|n| n as f32)
    };
    let bounds = Bounds::new(
        point(px(number("x")?), px(number("y")?)),
        size(px(number("width")?), px(number("height")?)),
    );
    Some(match value.get("state").and_then(|s| s.as_str()) {
        Some("zoomed") => WindowBounds::Maximized(bounds),
        // Full screen comes back as the window it was in, not full screen
        // at once: a Space of its own at launch surprised more than it
        // helped.
        _ => WindowBounds::Windowed(bounds),
    })
}

/// `bounds` as `window.json` keeps it.
fn written(bounds: WindowBounds) -> String {
    let (state, frame) = match bounds {
        WindowBounds::Windowed(frame) | WindowBounds::Fullscreen(frame) => ("windowed", frame),
        WindowBounds::Maximized(frame) => ("zoomed", frame),
    };
    serde_json::json!({
        "x": f32::from(frame.origin.x),
        "y": f32::from(frame.origin.y),
        "width": f32::from(frame.size.width),
        "height": f32::from(frame.size.height),
        "state": state,
    })
    .to_string()
}

/// Keeps `bounds` for the next start. Not being able to is not worth a
/// word: the window opens in the middle then.
pub fn keep(home: &Path, bounds: WindowBounds) {
    let _ = std::fs::create_dir_all(home);
    let _ = dm_core::write_atomic(&home.join(FILE), written(bounds).as_bytes());
}

/// `kept`, where its title bar is still on one of `displays` and it is at
/// least `least` in size; none otherwise.
pub fn on_a_display(
    kept: WindowBounds,
    displays: &[Bounds<Pixels>],
    least: Size<Pixels>,
) -> Option<WindowBounds> {
    let frame = kept.get_bounds();
    if frame.size.width < least.width || frame.size.height < least.height {
        return None;
    }
    // The middle of its title bar, where a person takes hold of it.
    let grip = point(
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + px(12.0),
    );
    displays
        .iter()
        .any(|display| display.contains(&grip))
        .then_some(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn a_frame_kept_reads_back_as_it_was() {
        let windowed = WindowBounds::Windowed(frame(120.0, 80.0, 900.0, 600.0));
        assert_eq!(read(&written(windowed)), Some(windowed));
        let zoomed = WindowBounds::Maximized(frame(0.0, 25.0, 1440.0, 875.0));
        assert_eq!(read(&written(zoomed)), Some(zoomed));
        let full = WindowBounds::Fullscreen(frame(10.0, 10.0, 800.0, 500.0));
        assert_eq!(
            read(&written(full)),
            Some(WindowBounds::Windowed(frame(10.0, 10.0, 800.0, 500.0)))
        );
        assert_eq!(read("{}"), None);
        assert_eq!(read("not json"), None);
    }

    #[test]
    fn a_frame_off_every_display_or_too_small_is_not_used() {
        let screen = [frame(0.0, 0.0, 1440.0, 900.0)];
        let least = size(px(640.0), px(400.0));
        let here = WindowBounds::Windowed(frame(100.0, 100.0, 900.0, 600.0));
        assert_eq!(on_a_display(here, &screen, least), Some(here));
        // On a display unplugged since.
        let gone = WindowBounds::Windowed(frame(2000.0, 100.0, 900.0, 600.0));
        assert_eq!(on_a_display(gone, &screen, least), None);
        let small = WindowBounds::Windowed(frame(100.0, 100.0, 300.0, 200.0));
        assert_eq!(on_a_display(small, &screen, least), None);
    }
}
