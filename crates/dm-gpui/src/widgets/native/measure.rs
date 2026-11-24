// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! How wide words are set: a form's label column measured by the system's text shaper, so a
//! label never begins outside its window (gaps row E2).

use super::TEXT;

/// The width a form's label column needs for `labels` at the system's
/// size: the widest, as the system's font sets these words, never under 60
/// points. Measured by the text system kept by `measure_with`; before it is
/// kept, guessed at 0.6 of the size a letter. The guess of 0.55 it once was
/// alone fell short by a tenth ("When this download is done": 186 points
/// for 203), and the words began outside the window (gaps row E2).
pub fn label_width(labels: &[&str]) -> f32 {
    labels
        .iter()
        .map(|l| words_width(l, TEXT))
        .fold(60.0, f32::max)
        .ceil()
}

thread_local! {
    /// The text system and the font the window's words are set in, as `measure_with` kept
    /// them: one for each thread an application runs on (a test's own).
    static MEASURE: std::cell::RefCell<Option<(std::sync::Arc<gpui::TextSystem>, gpui::FontId)>> =
        const { std::cell::RefCell::new(None) };
}

/// Keeps `cx`'s text system and the interface font, so `label_width` measures words as they
/// are drawn. Called once the application has started, after gpui-component's theme is set.
pub fn measure_with(cx: &gpui::App) {
    let family = gpui_component::ActiveTheme::theme(cx).font_family.clone();
    let system = cx.text_system().clone();
    let font = system.resolve_font(&gpui::font(family));
    MEASURE.with(|m| *m.borrow_mut() = Some((system, font)));
}

/// How wide `words` are set at `size` points, at 100%: each letter's advance in the
/// interface font, and a point over for what rounding gives back; guessed where nothing is
/// kept yet (`measure_with`).
fn words_width(words: &str, size: f32) -> f32 {
    let guess = || words.chars().count() as f32 * size * 0.6;
    MEASURE.with(|m| match &*m.borrow() {
        Some((system, font)) => {
            let mut width = 0.0;
            for ch in words.chars() {
                match system.advance(*font, gpui::px(size), ch) {
                    Ok(advance) => width += f32::from(advance.width),
                    Err(_) => return guess(),
                }
            }
            width + 1.0
        }
        None => guess(),
    })
}
