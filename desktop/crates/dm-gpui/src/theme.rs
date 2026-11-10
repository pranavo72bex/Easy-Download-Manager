// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's colours, from the system's own appearance (light or dark,
//! followed as the person switches it), as the system's own programs are
//! drawn: on macOS its window background and label colours. The font is the
//! system's (`.SystemUIFont`, GPUI's default).

use gpui::{rgb, rgba, Rgba, WindowAppearance};

/// The colours a view draws with.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// The window's background.
    pub background: Rgba,
    /// Text: the system's label colour.
    pub text: Rgba,
    /// Secondary text: the system's secondary label colour.
    pub muted: Rgba,
    /// Every other row's background, as the system's tables stripe them.
    pub stripe: Rgba,
    /// Lines between parts of the window.
    pub separator: Rgba,
}

impl Theme {
    /// The colours for the system's appearance.
    pub fn of(appearance: WindowAppearance) -> Theme {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme {
                // NSColor.windowBackgroundColor and labelColor, dark.
                background: rgb(0x1e1e1e),
                text: rgba(0xffffffd9),
                muted: rgba(0xffffff8c),
                stripe: rgba(0xffffff0a),
                separator: rgba(0xffffff1a),
            },
            WindowAppearance::Light | WindowAppearance::VibrantLight => Theme {
                background: rgb(0xffffff),
                text: rgba(0x000000d9),
                muted: rgba(0x00000080),
                stripe: rgba(0x0000000a),
                separator: rgba(0x0000001a),
            },
        }
    }
}
