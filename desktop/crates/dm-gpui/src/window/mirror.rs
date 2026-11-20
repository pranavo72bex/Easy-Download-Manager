// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The main window mirrored for a language read right to left, Arabic and Hebrew (gaps row
//! U35): the side panel on the right, the toolbar's buttons, the table's columns and the status
//! bar's parts from the right, text against the right. The one question the layout asks is
//! [`rtl`], the direction of the language in force (`i18n::direction`); every change it makes
//! is under it, so a window in a left-to-right language is laid out as it always was. The
//! words themselves are laid out right to left by the system's text (CoreText, DirectWrite
//! with dm's `dwrite-rtl`, cosmic-text). No icon of the main window points left or right (its
//! arrows point up and down), so none is turned.

use gpui::Styled;

/// Whether the window is laid out from the right: the language in force reads right to left.
pub fn rtl() -> bool {
    dm_desktop_base::i18n::direction().is_rtl()
}

/// `row`, a flex row, laid out from the right where the window is mirrored, and as it was
/// otherwise. Given after `.flex()`; never to a column.
pub fn row<E: Styled>(row: E) -> E {
    if rtl() {
        row.flex_row_reverse()
    } else {
        row
    }
}
