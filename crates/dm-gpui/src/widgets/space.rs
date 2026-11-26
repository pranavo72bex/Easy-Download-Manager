// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's spacing, lines and corners, named once (gaps row A19): every gap, padding and
//! margin is a step of this scale, so two parts of the window that should sit alike do, and a
//! step changed here moves every place that uses it. What the system's own dialogs measure
//! (their margins, a row's gap, a control's height) is in [`super::native`], per system; a
//! control's own width or height stays beside the control.
//!
//! In points, at 100 %; `crate::window::zoom::px` scales them with the window's zoom.

/// Two points: a hairline's room, text a little off its neighbour.
pub const XXS: f32 = 2.0;
/// Four points: an icon and its label, the rows of a small list.
pub const XS: f32 = 4.0;
/// Six points.
pub const S: f32 = 6.0;
/// Eight points: the common gap between the parts of a bar or a row.
pub const M: f32 = 8.0;
/// Ten points.
pub const M_L: f32 = 10.0;
/// Twelve points: a pill's or a button's sides.
pub const L: f32 = 12.0;
/// Sixteen points.
pub const XL: f32 = 16.0;
/// Twenty points.
pub const XXL: f32 = 20.0;

/// Forty points: room above an empty list's words, a section set apart.
pub const SECTION: f32 = 40.0;

/// Forty-eight points: a finger's least target at a phone's width (gaps row N7), as phones'
/// own guidelines ask (44 to 48).
pub const TOUCH: f32 = 48.0;

/// A rule's or a border's thickness, and the gap that shows one between two parts.
pub const LINE: f32 = 1.0;

/// A small part's corners (a badge, a pill, a menu's row).
pub const CORNER_S: f32 = 4.0;
/// A popup's corners.
pub const CORNER_L: f32 = 8.0;

#[cfg(test)]
mod tests {
    /// No text size, gap, padding or margin is written as a bare number outside `widgets/` and
    /// the theme: each is a step of this scale or of the type ramp (`native::TEXT` and the
    /// sizes beside it), so a step changed there moves every place (gaps row V13). A control's
    /// own width or height may stay beside it.
    #[test]
    fn no_bare_size_outside_the_widgets() {
        let sizes = [
            "text_size",
            "gap",
            "gap_x",
            "gap_y",
            "p",
            "px",
            "py",
            "pt",
            "pb",
            "pl",
            "pr",
            "m",
            "mx",
            "my",
            "mt",
            "mb",
            "ml",
            "mr",
        ];
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut bare = Vec::new();
        let mut folders = vec![src.clone()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = entry.path();
                let rel = path
                    .strip_prefix(&src)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if rel.starts_with("widgets") || rel.starts_with("window/theme") {
                    continue;
                }
                if path.is_dir() {
                    folders.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                for (n, line) in text.lines().enumerate() {
                    for size in sizes {
                        let call = format!(".{size}(px(");
                        let Some(at) = line.find(&call) else { continue };
                        let rest = &line[at + call.len()..];
                        let number: String = rest
                            .chars()
                            .take_while(|c| c.is_ascii_digit() || *c == '.')
                            .collect();
                        let bare_number =
                            !number.is_empty() && rest[number.len()..].starts_with(')');
                        if bare_number && !number.trim_end_matches(['0', '.']).is_empty() {
                            bare.push(format!("{rel}:{}: {}", n + 1, line.trim()));
                        }
                    }
                }
            }
        }
        assert!(
            bare.is_empty(),
            "bare sizes; name them in space.rs or native.rs:\n{}",
            bare.join("\n")
        );
    }
}

/// The window's layers, back to front (gaps row E48): a layer drawn over the window takes its
/// place from here, not a number guessed beside another one's.
pub mod layer {
    /// A menu over the window (the place menu).
    pub const MENU: usize = 1;
    /// A bottom sheet over the window and its menus (a pop-up's choices on a phone).
    pub const SHEET: usize = 10;
}

#[cfg(test)]
mod layer_tests {
    use super::layer;

    /// A sheet is drawn over a menu (gaps row E48).
    #[test]
    fn layers_stack_in_their_named_order() {
        const { assert!(layer::MENU < layer::SHEET) };
    }
}
