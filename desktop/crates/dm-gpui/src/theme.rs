// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's colours, from the system's own appearance (light or dark,
//! followed as the person switches it), as each system's own programs are
//! drawn: macOS's window background and label colours (AppKit's), Windows
//! 11's (WinUI's Fluent colours: Mica's grey, its blue accent, Segoe's
//! black at 89%), and on Linux GNOME's (libadwaita's). The font is the
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
    /// Words and marks on the accent.
    pub on_accent: Rgba,
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

    /// The colours for the system's appearance, as this system draws them.
    pub fn of(appearance: WindowAppearance) -> Theme {
        let dark = matches!(
            appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        if cfg!(windows) {
            windows(dark)
        } else if cfg!(target_os = "macos") {
            macos(appearance)
        } else {
            gnome(dark)
        }
    }
}

/// macOS's: AppKit's colours, measured on its own windows.
fn macos(appearance: WindowAppearance) -> Theme {
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
            on_accent: rgb(0xffffff),
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
            on_accent: rgb(0xffffff),
        },
    }
}

/// Windows 11's: WinUI's Fluent colours (Mica's grey under the window, a
/// list on the layer above it, the default blue accent, text in black or
/// white at Fluent's opacities).
fn windows(dark: bool) -> Theme {
    if dark {
        Theme {
            background: rgb(0x272727),
            text: rgb(0xffffff),
            muted: rgba(0xffffffc8),
            stripe: rgba(0xffffff08),
            separator: rgba(0xffffff15),
            sidebar: rgb(0x202020),
            selection: rgb(0x60cdff),
            on_selection: rgb(0x000000),
            menu: rgb(0x2c2c2c),
            warning: rgb(0xff99a4),
            control: rgba(0xffffff0f),
            control_border: rgba(0xffffff17),
            field: rgb(0x2d2d2d),
            window: rgb(0x202020),
            accent: rgb(0x60cdff),
            on_accent: rgb(0x000000),
            success: rgb(0x6ccb5f),
        }
    } else {
        Theme {
            background: rgb(0xfbfbfb),
            text: rgba(0x000000e4),
            muted: rgba(0x0000009e),
            stripe: rgba(0x00000008),
            separator: rgba(0x00000014),
            sidebar: rgb(0xf3f3f3),
            selection: rgb(0x005fb8),
            on_selection: rgb(0xffffff),
            menu: rgb(0xf9f9f9),
            warning: rgb(0xc42b1c),
            control: rgb(0xffffff),
            control_border: rgba(0x00000024),
            field: rgb(0xffffff),
            window: rgb(0xf3f3f3),
            accent: rgb(0x005fb8),
            on_accent: rgb(0xffffff),
            success: rgb(0x0f7b0f),
        }
    }
}

/// GNOME's: libadwaita's colours (its window, sidebar and view colours,
/// its blue accent, its destructive red and success green).
fn gnome(dark: bool) -> Theme {
    if dark {
        Theme {
            background: rgb(0x1e1e1e),
            text: rgb(0xffffff),
            muted: rgba(0xffffffb3),
            stripe: rgba(0xffffff08),
            separator: rgba(0xffffff17),
            sidebar: rgb(0x2e2e32),
            selection: rgb(0x3584e4),
            on_selection: rgb(0xffffff),
            menu: rgb(0x36363a),
            warning: rgb(0xff7b63),
            control: rgba(0xffffff1a),
            control_border: rgba(0x00000000),
            field: rgba(0xffffff1a),
            window: rgb(0x222226),
            accent: rgb(0x3584e4),
            on_accent: rgb(0xffffff),
            success: rgb(0x78e9ab),
        }
    } else {
        Theme {
            background: rgb(0xffffff),
            text: rgba(0x000000cc),
            muted: rgba(0x0000008c),
            stripe: rgba(0x00000008),
            separator: rgba(0x00000017),
            sidebar: rgb(0xebebed),
            selection: rgb(0x3584e4),
            on_selection: rgb(0xffffff),
            menu: rgb(0xffffff),
            warning: rgb(0xc01c28),
            control: rgba(0x0000001a),
            control_border: rgba(0x00000000),
            field: rgba(0x0000000f),
            window: rgb(0xfafafb),
            accent: rgb(0x3584e4),
            on_accent: rgb(0xffffff),
            success: rgb(0x1b8553),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG's contrast between two opaque colours, `fore` blended onto `back`.
    fn contrast(fore: Rgba, back: Rgba) -> f32 {
        let blend = |f: f32, b: f32| f * fore.a + b * (1.0 - fore.a);
        let lum = |c: [f32; 3]| {
            let l = |v: f32| {
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * l(c[0]) + 0.7152 * l(c[1]) + 0.0722 * l(c[2])
        };
        let f = lum([
            blend(fore.r, back.r),
            blend(fore.g, back.g),
            blend(fore.b, back.b),
        ]);
        let b = lum([back.r, back.g, back.b]);
        (f.max(b) + 0.05) / (f.min(b) + 0.05)
    }

    #[test]
    fn every_systems_palette_keeps_its_words_readable() {
        let mut all = Vec::new();
        for dark in [false, true] {
            all.push(("Windows", dark, windows(dark)));
            all.push(("GNOME", dark, gnome(dark)));
            let appearance = if dark {
                WindowAppearance::Dark
            } else {
                WindowAppearance::Light
            };
            all.push(("macOS", dark, macos(appearance)));
        }
        for (system, dark, theme) in all {
            let at = format!("{system}, dark {dark}");
            assert!(contrast(theme.text, theme.background) >= 7.0, "{at}: text");
            assert!(contrast(theme.text, theme.sidebar) >= 7.0, "{at}: sidebar");
            assert!(
                contrast(theme.muted, theme.background) >= 3.0,
                "{at}: muted"
            );
            assert!(
                contrast(theme.on_selection, theme.selection) >= 3.0,
                "{at}: a chosen row"
            );
            assert!(
                contrast(theme.on_accent, theme.accent) >= 3.0,
                "{at}: default button"
            );
        }
    }
}
