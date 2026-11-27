// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Right-to-left words that wrap, in reading order (gaps row E52). GPUI breaks a wrapped line
//! by walking its glyphs from the left of the screen (`LineLayout::compute_wrap_boundaries`),
//! which for Arabic or Hebrew is the sentence's end: its first line held the last words and
//! the sentence began on the last line (Options' hints at 400 points). [`Paragraph`] breaks
//! the words itself, in the order they are read, at their spaces, and lets the system lay each
//! line out right to left. A line that fits, and words that do not wrap (cut with "…", kept
//! on one line), are GPUI's own text element, as everywhere else.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    prelude::*, AnyElement, App, AvailableSpace, Bounds, GlobalElementId, InspectorElementId,
    LayoutId, Pixels, SharedString, Size, WhiteSpace, Window,
};

/// Whether `text`, drawn now, is laid out by [`Paragraph`]: the language reads right to left
/// and the words hold a letter of Arabic or Hebrew.
pub fn needs(text: &str) -> bool {
    crate::window::mirror::rtl() && text.chars().any(right_to_left)
}

/// A letter of a script read right to left (Hebrew, Arabic, Syriac, Thaana, N'Ko and their
/// presentation forms).
fn right_to_left(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

/// The words, broken at their spaces into lines no wider than `wrap` by `width`, in reading
/// order. A word wider than the line stands on a line of its own. Each `\n` starts a line.
fn lines(text: &str, wrap: Option<Pixels>, mut width: impl FnMut(&str) -> Pixels) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let Some(wrap) = wrap else {
            out.push(paragraph.to_string());
            continue;
        };
        let mut line = String::new();
        for word in paragraph.split_inclusive(' ') {
            let longer = format!("{line}{word}");
            if line.is_empty() || width(longer.trim_end()) <= wrap {
                line = longer;
            } else {
                out.push(line.trim_end().to_string());
                line = word.to_string();
            }
        }
        out.push(line.trim_end().to_string());
    }
    out
}

/// `text` drawn as GPUI's text element draws it, its lines broken in reading order.
pub struct Paragraph(pub SharedString);

/// What the layout found: the lines and their height, or GPUI's own element.
pub enum Laid {
    Lines(Rc<RefCell<(Vec<SharedString>, Pixels)>>),
    Plain(AnyElement),
}

impl IntoElement for Paragraph {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Paragraph {
    type RequestLayoutState = Laid;
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Laid) {
        let style = window.text_style();
        // Words cut or kept on one line are GPUI's: nothing of them wraps.
        if style.white_space != WhiteSpace::Normal
            || style.text_overflow.is_some()
            || style.line_clamp.is_some()
        {
            let mut plain = self.0.clone().into_any_element();
            let id = plain.request_layout(window, cx);
            return (id, Laid::Plain(plain));
        }
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.pixel_snap(
            style
                .line_height
                .to_pixels(font_size.into(), window.rem_size()),
        );
        let laid = Rc::new(RefCell::new((Vec::new(), line_height)));
        let text = self.0.clone();
        let id = window.request_measured_layout(Default::default(), {
            let laid = Rc::clone(&laid);
            move |known, available, window, _| {
                let wrap = known.width.or(match available.width {
                    AvailableSpace::Definite(x) => Some(x),
                    _ => None,
                });
                let shape = |line: &str, window: &mut Window| {
                    let line = SharedString::from(line.to_string());
                    let run = style.to_run(line.len());
                    window
                        .text_system()
                        .shape_line(line, font_size, &[run], None)
                        .width
                };
                let broken = lines(&text, wrap, |line| shape(line, window));
                let widest = broken
                    .iter()
                    .map(|line| shape(line, window))
                    .fold(Pixels::ZERO, Pixels::max)
                    // Whole points, as GPUI's own text measures itself.
                    .ceil();
                let height = line_height * broken.len() as f32;
                laid.borrow_mut().0 = broken.into_iter().map(SharedString::from).collect();
                Size {
                    width: widest,
                    height,
                }
            }
        });
        (id, Laid::Lines(laid))
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        laid: &mut Laid,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Laid::Plain(plain) = laid {
            plain.prepaint(window, cx);
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        laid: &mut Laid,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let laid = match laid {
            Laid::Plain(plain) => return plain.paint(window, cx),
            Laid::Lines(laid) => laid.borrow(),
        };
        let (lines, line_height) = &*laid;
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let mut origin = bounds.origin;
        for line in lines {
            let run = style.to_run(line.len());
            let shaped = window
                .text_system()
                .shape_line(line.clone(), font_size, &[run], None);
            // A line that cannot be drawn (no font) is left out, as GPUI's own text leaves it.
            let _ = shaped.paint(
                origin,
                *line_height,
                style.text_align,
                Some(bounds.size.width),
                window,
                cx,
            );
            origin.y += *line_height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every letter 1 point wide.
    fn letters(line: &str) -> Pixels {
        gpui::px(line.chars().count() as f32)
    }

    #[test]
    fn words_are_broken_in_reading_order_at_their_spaces() {
        let text = "الأول الثاني الثالث";
        assert_eq!(
            lines(text, Some(gpui::px(12.0)), letters),
            ["الأول الثاني", "الثالث"]
        );
        assert_eq!(lines(text, None, letters), [text]);
        // A word wider than the line stands alone; a line break starts a line.
        assert_eq!(
            lines("a verylongword b\nc", Some(gpui::px(3.0)), letters),
            ["a", "verylongword", "b", "c"]
        );
    }

    #[test]
    fn only_right_to_left_letters_are_marked() {
        assert!(right_to_left('ا'));
        assert!(right_to_left('ש'));
        assert!(!right_to_left('a'));
        assert!(!right_to_left('5'));
    }
}
