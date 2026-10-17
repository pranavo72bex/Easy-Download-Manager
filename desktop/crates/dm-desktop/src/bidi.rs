//! Names in Arabic and Hebrew, laid out in the order they are read.
//!
//! egui draws a line's characters left to right in the order they are
//! stored. Its shaper turns each run of one font that is in a right-to-left
//! script round, so a single Arabic or Hebrew word comes out right, but the
//! words of a right-to-left phrase, and the brackets and spaces between
//! them, stay in stored order: "ملف عربي.zip" was drawn with its two words
//! swapped. `visual` puts a name into the order it is drawn in, by the
//! Unicode bidirectional algorithm on a left-to-right line (dm's window
//! is one), and leaves each right-to-left word in stored order for the
//! shaper to turn as it already does.
//!
//! Only what is drawn changes: a screen reader is still given the name as
//! stored, which is the order it reads in (`label`).

// Reads outside input (file names and paths): every index is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::borrow::Cow;

use unicode_bidi::{bidi_class, BidiClass, BidiInfo, Level};

/// Whether `c` is a letter of a right-to-left script, or a mark on one: what
/// egui's shaper turns round itself.
fn turned_by_shaper(c: char) -> bool {
    matches!(bidi_class(c), BidiClass::R | BidiClass::AL | BidiClass::NSM)
}

/// Whether any character of `text` is written right to left.
fn has_rtl(text: &str) -> bool {
    !text.is_ascii()
        && text
            .chars()
            .any(|c| matches!(bidi_class(c), BidiClass::R | BidiClass::AL))
}

/// A bracket drawn in a right-to-left run faces the other way.
fn mirrored(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        c => c,
    }
}

/// `text` in the order egui is to draw it so that it reads as it should:
/// unchanged where nothing in it is right to left, which is nearly every
/// name.
pub fn visual(text: &str) -> Cow<'_, str> {
    if !has_rtl(text) {
        return Cow::Borrowed(text);
    }
    let info = BidiInfo::new(text, Some(Level::ltr()));
    let mut out = String::with_capacity(text.len());
    for para in &info.paragraphs {
        let line = para.range.clone();
        let (levels, runs) = info.visual_runs(para, line);
        for run in runs {
            let Some(part) = text.get(run.clone()) else {
                continue;
            };
            let rtl = levels.get(run.start).is_some_and(|l| l.is_rtl());
            if rtl {
                right_to_left(part, &mut out);
            } else {
                out.push_str(part);
            }
        }
    }
    Cow::Owned(out)
}

/// One right-to-left run, drawn left to right: its pieces in reverse order,
/// each word of a right-to-left script kept as stored (the shaper turns it
/// round), every other character on its own, mirrored where it is a bracket.
fn right_to_left(part: &str, out: &mut String) {
    let mut pieces: Vec<Cow<'_, str>> = Vec::new();
    let mut word = String::new();
    for c in part.chars() {
        if turned_by_shaper(c) {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            pieces.push(Cow::Owned(std::mem::take(&mut word)));
        }
        pieces.push(Cow::Owned(mirrored(c).to_string()));
    }
    if !word.is_empty() {
        pieces.push(Cow::Owned(word));
    }
    for piece in pieces.iter().rev() {
        out.push_str(piece);
    }
}

/// Gives a label drawn from `visual(stored)` the stored text as its name
/// for a screen reader, which reads in stored order.
pub fn said_as(response: &egui::Response, stored: &str) {
    if has_rtl(stored) {
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, stored));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gap 130: the words of an Arabic or Hebrew name are drawn in the
    /// order they are read; each word stays as stored, which egui's shaper
    /// turns round itself.
    #[test]
    fn right_to_left_words_are_drawn_in_reading_order() {
        // "file Arabic" in Arabic, then ".zip": drawn with "Arabic" first,
        // at the left, as a browser draws it on a left-to-right line.
        assert_eq!(visual("ملف عربي.zip"), "عربي ملف.zip");
        assert_eq!(visual("קובץ עברי.zip"), "עברי קובץ.zip");
        // A Latin name inside, and a number, keep their own order.
        assert_eq!(visual("report تقرير 2026.pdf"), "report 2026 تقرير.pdf");
        // Brackets in a right-to-left run face the other way.
        assert_eq!(visual("(ملف عربي)"), "(عربي ملف)");
        // A path with a Hebrew folder: the folders stay in their order.
        assert_eq!(visual("/home/u/קובץ עברי/a.zip"), "/home/u/עברי קובץ/a.zip");
    }

    #[test]
    fn a_name_with_nothing_right_to_left_is_left_as_it_is() {
        for name in [
            "report.pdf",
            "名前 ファイル.zip",
            "Résumé – final (copy) [2].pdf",
            "",
        ] {
            assert!(matches!(visual(name), Cow::Borrowed(n) if n == name));
        }
    }
}
