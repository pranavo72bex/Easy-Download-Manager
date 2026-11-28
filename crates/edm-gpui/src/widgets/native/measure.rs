// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
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

/// How wide `words` are set in the system's size for controls and labels, in points at 100%.
pub fn text_width(words: &str) -> f32 {
    words_width(words, TEXT)
}

/// How wide `words` are set in the system's small size (a toolbar button's word), in points
/// at 100%.
pub fn small_width(words: &str) -> f32 {
    words_width(words, super::SMALL_TEXT)
}

/// The font's figures all one width ("tnum"), for figures that change as they are read: a
/// speed, the time left, a percentage. Proportional figures shifted the words after them at
/// every tick (gaps row E20).
pub fn figures() -> gpui::FontFeatures {
    gpui::FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]))
}

thread_local! {
    /// The text system and the font the window's words are set in, as `measure_with` kept
    /// them: one for each thread an application runs on (a test's own).
    static MEASURE: std::cell::RefCell<Option<Measure>> = const { std::cell::RefCell::new(None) };
}

/// What words are measured with: the text system, the interface font and its id, and a line
/// shaper of its own (`WindowTextSystem`), which sets a run of letters as it is drawn.
struct Measure {
    system: std::sync::Arc<gpui::TextSystem>,
    font_id: gpui::FontId,
    font: gpui::Font,
    lines: gpui::WindowTextSystem,
}

/// Keeps `cx`'s text system and the interface font, so `label_width` measures words as they
/// are drawn. Called once the application has started, after gpui-component's theme is set.
pub fn measure_with(cx: &gpui::App) {
    let family = gpui_component::ActiveTheme::theme(cx).font_family.clone();
    let system = cx.text_system().clone();
    let font = gpui::font(family);
    let font_id = system.resolve_font(&font);
    let lines = gpui::WindowTextSystem::new(system.clone());
    MEASURE.with(|m| {
        *m.borrow_mut() = Some(Measure {
            system,
            font_id,
            font,
            lines,
        })
    });
}

/// How wide `words` are set at `size` points, at 100%: each letter as the system shapes it in
/// the interface font, and a point over for what rounding gives back; guessed where nothing is
/// kept yet (`measure_with`). Shaped rather than read from the font's advances: macOS sets its
/// system font with tracking at small sizes the advances leave out, and labels came out a
/// twentieth wider than measured ("Delete file after" stood out of its column, gaps row E10).
/// Kept once measured, as a form's labels are measured at each frame.
fn words_width(words: &str, size: f32) -> f32 {
    let guess = || words.chars().count() as f32 * size * 0.6;
    if let Some(known) = MEASURED.with(|m| {
        m.borrow()
            .get(&(words.to_string(), size.to_bits()))
            .copied()
    }) {
        return known;
    }
    // Each letter alone, and the words set as one run, the wider: letters that join (Arabic)
    // take other shapes in a run than alone, and "305.4 ك.ب/ث" measured letter by letter fell
    // short and was cut; Latin letters set alone are no narrower than in a run.
    let measured = MEASURE.with(|m| {
        m.borrow().as_ref().map(|measure| {
            let alone = words
                .chars()
                .map(|ch| {
                    f32::from(
                        measure
                            .system
                            .layout_width(measure.font_id, gpui::px(size), ch),
                    )
                })
                .sum::<f32>();
            let run = gpui::TextRun {
                len: words.len(),
                font: measure.font.clone(),
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let set = f32::from(
                measure
                    .lines
                    .layout_line(words, gpui::px(size), &[run], None)
                    .width,
            );
            alone.max(set) + 1.0
        })
    });
    match measured {
        Some(width) => {
            MEASURED.with(|m| {
                m.borrow_mut()
                    .insert((words.to_string(), size.to_bits()), width)
            });
            width
        }
        None => guess(),
    }
}

thread_local! {
    /// Words measured, by their size: a form's labels at each frame cost one look each.
    static MEASURED: std::cell::RefCell<std::collections::HashMap<(String, u32), f32>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}
