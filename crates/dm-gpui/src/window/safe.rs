// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What of a window the system covers, kept clear of dm's controls (gaps row N16). On a phone
//! every window is drawn over the whole screen, under the status bar, the navigation bar and
//! a camera's cut-out, and the platform tells what they cover as GPUI's insets
//! (`Window::insets`; the soft keyboard's part with them where the platform tells it). A
//! window's root is padded by them, so its controls sit where they can be seen and touched.
//! The desktops' platforms tell no insets (GPUI's default, all zero), so on a desktop nothing
//! moves.

use gpui::{Edges, Pixels, Styled, Window};

/// What the system covers on each side of `window`, in logical points: zero on a desktop.
pub(crate) fn edges(window: &Window) -> Edges<Pixels> {
    window.insets().effective()
}

/// `element` (a window's root) padded clear of what the system covers.
pub(crate) fn kept_clear<E: Styled>(element: E, window: &Window) -> E {
    padded(element, edges(window))
}

/// `element` padded by `at` on each side.
fn padded<E: Styled>(element: E, at: Edges<Pixels>) -> E {
    element.pt(at.top).pr(at.right).pb(at.bottom).pl(at.left)
}

#[cfg(test)]
mod tests {
    use gpui::{div, px, AbsoluteLength, DefiniteLength, Edges, Styled};

    use super::padded;

    fn points(value: f32) -> Option<DefiniteLength> {
        Some(DefiniteLength::Absolute(AbsoluteLength::Pixels(px(value))))
    }

    /// Each side padded by what covers it: a status bar above, a navigation bar below, a
    /// cut-out at one side.
    #[test]
    fn each_side_is_padded_by_what_covers_it() {
        let mut root = padded(
            div(),
            Edges {
                top: px(24.0),
                right: px(0.0),
                bottom: px(48.0),
                left: px(30.0),
            },
        );
        let padding = &root.style().padding;
        assert_eq!(padding.top, points(24.0));
        assert_eq!(padding.right, points(0.0));
        assert_eq!(padding.bottom, points(48.0));
        assert_eq!(padding.left, points(30.0));
    }
}
