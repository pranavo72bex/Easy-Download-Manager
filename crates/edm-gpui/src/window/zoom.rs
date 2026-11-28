// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's scale (View › Text size, Options › General › Appearance's
//! slider: 60, 80, 100 or 120 percent): one scale for everything EDM draws.
//! GPUI has no zoom of its own and every measure EDM gives it is in points, so
//! each one goes through [`px`] here, which multiplies it by the size in
//! force: text, rows, controls, paddings and pictures grow together. What
//! gpui-component draws in rems follows through its theme's font size, which
//! sets each window's rem. A position or size the system reports (a
//! window's frame, the screen's, the pointer's) is already in points on
//! screen and is used with `gpui::px` as it is.
//!
//! The main window's least size stays the same on screen at every text size:
//! grown with it, a window at 200% could not be made small enough to fit a
//! laptop's screen. A dialog's window is resized by the same factor as the
//! text, so what it holds still fits.

use std::sync::atomic::{AtomicU32, Ordering};

use edm_shell::settings::TEXT_SIZE;
use gpui::{App, Pixels};

/// The size in force, as an `f32`'s bits: read on every measure of every
/// frame, from any window.
#[cfg(not(test))]
static FACTOR: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0

// Each test's own, as the language is (`i18n::set`): a window test drives its window on its own
// thread, and one at 200% (the goldens', the sweep's) laid every window of the tests running
// beside it out at 200%, where a press missed what it aimed at (the Grabber's test, twice).
#[cfg(test)]
thread_local! {
    static FACTOR: AtomicU32 = const { AtomicU32::new(0x3f80_0000) };
}

/// The size in force's bits, read.
fn load() -> u32 {
    #[cfg(test)]
    return FACTOR.with(|f| f.load(Ordering::Relaxed));
    #[cfg(not(test))]
    FACTOR.load(Ordering::Relaxed)
}

/// The size in force's bits, set.
fn store(bits: u32) {
    #[cfg(test)]
    FACTOR.with(|f| f.store(bits, Ordering::Relaxed));
    #[cfg(not(test))]
    FACTOR.store(bits, Ordering::Relaxed);
}

/// gpui-component's own rem, at 100% (its theme's default font size).
const REM: f32 = 16.0;

/// The scale in force: 1.0 is 100%.
pub fn factor() -> f32 {
    f32::from_bits(load())
}

/// `points` of EDM's design, at the text size in force.
pub fn px(points: f32) -> Pixels {
    gpui::px(of(points))
}

/// `points` of EDM's design as a number, at the text size in force: for
/// arithmetic with what the system reports in points on screen.
pub fn of(points: f32) -> f32 {
    at(points, factor())
}

/// `points` at the text size `size`.
fn at(points: f32, size: f32) -> f32 {
    points * size
}

/// Puts `size` in force (held to the nearest stop) before any window is
/// drawn: the theme's rem, nothing redrawn. For the start.
pub fn start(size: f32, cx: &mut App) {
    store(TEXT_SIZE.held(size).to_bits());
    rem(cx);
}

/// Puts `size` in force in every window at once, without a restart: each
/// dialog's window resized by as much as the text changed, so what it holds
/// fits as before, and every window drawn again. Whether it changed.
pub fn set(size: f32, cx: &mut App) -> bool {
    let size = TEXT_SIZE.held(size);
    let was = factor();
    if (size - was).abs() < 0.001 {
        return false;
    }
    store(size.to_bits());
    rem(cx);
    let ratio = size / was;
    for (handle, _, dialog) in crate::window::screen::Screens::all(cx) {
        if !dialog {
            continue;
        }
        // After this update: a window busy drawing cannot be resized.
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, _| {
                let now = window.viewport_size();
                window.resize(gpui::size(now.width * ratio, now.height * ratio));
            });
        });
    }
    cx.refresh_windows();
    true
}

/// gpui-component's font size, which `Root` makes each window's rem: its
/// own controls drawn in rems scale with EDM's.
fn rem(cx: &mut App) {
    if cx.has_global::<gpui_component::Theme>() {
        let size = gpui::px(REM * factor());
        gpui_component::Theme::global_mut(cx).font_size = size;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 100% until a scale is put in force, and a measure grows and
    /// shrinks with the size (the global is left alone: the tests run
    /// side by side).
    #[test]
    fn a_measure_follows_the_text_size() {
        assert_eq!(f32::from_bits(0x3f80_0000), 1.0);
        assert_eq!(factor(), 1.0);
        assert_eq!(px(10.0), gpui::px(10.0));
        assert!((at(24.0, 1.2) - 28.8).abs() < 0.001);
        assert!((at(13.0, 0.85) - 11.05).abs() < 0.001);
    }
}
