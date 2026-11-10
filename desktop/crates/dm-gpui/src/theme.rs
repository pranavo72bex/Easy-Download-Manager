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
    /// The side panel's background, as a source list's.
    pub sidebar: Rgba,
    /// The chosen item's background: the system's selection colour.
    pub selection: Rgba,
    /// Text on the chosen item.
    pub on_selection: Rgba,
    /// A menu's background.
    pub menu: Rgba,
    /// What a dialog says is wrong: the system's red.
    pub warning: Rgba,
    /// A control's face: a push button's, a pop-up's.
    pub control: Rgba,
    /// A control's edge, where the system draws one (light).
    pub control_border: Rgba,
    /// A text field's inside.
    pub field: Rgba,
    /// A dialog's, a sheet's and the window's own background
    /// (`NSColor.windowBackgroundColor`); `background` is a list's.
    pub window: Rgba,
    /// The default button's face: the system's accent.
    pub accent: Rgba,
    /// What is in order (an extension that has spoken to dm): the system's
    /// green.
    pub success: Rgba,
}

impl Theme {
    /// The colours on a chosen row: its words and marks on the selection
    /// colour.
    pub fn chosen(self) -> Theme {
        Theme {
            text: self.on_selection,
            muted: self.on_selection,
            selection: self.on_selection,
            ..self
        }
    }

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
                sidebar: rgb(0x262626),
                // NSColor.selectedContentBackgroundColor with the blue accent.
                selection: rgb(0x0a5fd8),
                on_selection: rgb(0xffffff),
                menu: rgb(0x2c2c2c),
                // NSColor.systemRed, dark.
                warning: rgb(0xff453a),
                // Measured on macOS's own dialog, dark: a push button is
                // white at 8% over the window, with no edge; a text field
                // nearly black; the window #2e2c2b (tinted by the
                // wallpaper; untinted, #2d2d2d).
                control: rgba(0xffffff14),
                control_border: rgba(0x00000000),
                field: rgb(0x1a1a1a),
                window: rgb(0x2d2d2d),
                accent: rgb(0x007aff),
                // NSColor.systemGreen, dark.
                success: rgb(0x30d158),
            },
            WindowAppearance::Light | WindowAppearance::VibrantLight => Theme {
                background: rgb(0xffffff),
                text: rgba(0x000000d9),
                muted: rgba(0x00000080),
                stripe: rgba(0x0000000a),
                separator: rgba(0x0000001a),
                sidebar: rgb(0xf0f0f0),
                selection: rgb(0x0064e1),
                on_selection: rgb(0xffffff),
                menu: rgb(0xf6f6f6),
                warning: rgb(0xff3b30),
                control: rgb(0xffffff),
                control_border: rgba(0x00000026),
                field: rgb(0xffffff),
                window: rgb(0xececec),
                accent: rgb(0x007aff),
                success: rgb(0x34c759),
            },
        }
    }
}

/// The look Options › General asks for: Light or Dark whatever the
/// computer's, else the computer's (`classic` is the egui window's own look,
/// drawn here as the computer's). Every window follows, as the system's own
/// setting would have it.
pub fn follow(choice: &str, cx: &mut gpui::App) {
    cx.set_window_appearance(match choice {
        "light" => Some(WindowAppearance::Light),
        "dark" => Some(WindowAppearance::Dark),
        _ => None,
    });
}
