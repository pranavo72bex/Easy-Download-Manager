// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's colours, from the system's own appearance (light or dark,
//! followed as the person switches it), as each system's own programs are
//! drawn: macOS's window background and label colours (AppKit's), Windows
//! 11's (WinUI's Fluent colours: Mica's grey, its blue accent, Segoe's
//! black at 89%), and on Linux GNOME's (libadwaita's); under Windows' High
//! Contrast, the system's own colours. The font is the system's
//! (`.SystemUIFont`, GPUI's default).

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

    /// Whether these are a chosen row's colours (`chosen`): there the selection is drawn in the
    /// words' own colour, which no system's palette does (it would hide what is chosen).
    pub fn is_chosen(&self) -> bool {
        self.selection == self.on_selection && self.text == self.on_selection
    }

    /// The colours for the system's appearance, as this system draws them.
    /// Under Windows' High Contrast, its own colours, whatever the look
    /// (contract B45: "on every look (light, dark, high contrast)").
    pub fn of(appearance: WindowAppearance) -> Theme {
        if let Some(palette) = dm_desktop_base::contrast::system() {
            return high_contrast(&palette);
        }
        let dark = matches!(
            appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let own = if cfg!(windows) {
            windows(dark)
        } else if cfg!(target_os = "macos") {
            macos(dark)
        } else {
            gnome(dark)
        };
        match dm_desktop_base::accent::system() {
            Some(accent) => own.with_accent(&accent, dark),
            None => own,
        }
    }

    /// These colours with the system's accent where buttons and the selection take it, as the
    /// system's own programs draw them (gaps row U33): in the light look its darker shade, in
    /// the dark look its lighter one. An accent that would leave the words on it hard to read
    /// (under 4.5 to 1) keeps dm's own.
    pub fn with_accent(self, accent: &dm_desktop_base::accent::Palette, dark: bool) -> Theme {
        let [r, g, b] = if dark { accent.light1 } else { accent.dark1 };
        let shade = rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b));
        let readable = |words: Rgba| words_on(words, shade) >= 4.5;
        if readable(self.on_accent) && readable(self.on_selection) {
            Theme {
                accent: shade,
                selection: shade,
                ..self
            }
        } else {
            self
        }
    }
}

/// WCAG's contrast of `words` laid over the opaque `back`.
fn words_on(words: Rgba, back: Rgba) -> f32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let on = |f: f32, b: f32| byte(f * words.a + b * (1.0 - words.a));
    dm_desktop_base::contrast::ratio(
        [
            on(words.r, back.r),
            on(words.g, back.g),
            on(words.b, back.b),
        ],
        [byte(back.r), byte(back.g), byte(back.b)],
    )
}

/// macOS's: AppKit's colours, measured on its own windows.
fn macos(dark: bool) -> Theme {
    if dark {
        Theme {
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
            // NSColor.systemRed, dark, as macOS draws it with Increase contrast: the plain one
            // read 4.0 to 1 on a dialog (gaps row U22).
            warning: rgb(0xff6961),
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
        }
    } else {
        Theme {
            background: rgb(0xffffff),
            text: rgba(0x000000d9),
            // Darker than labelColor's secondary (50%), which read 3.8 to 1 on a dialog; WCAG's
            // 4.5 on every surface (gaps row U22).
            muted: rgba(0x00000099),
            stripe: rgba(0x0000000a),
            separator: rgba(0x0000001a),
            sidebar: rgb(0xf0f0f0),
            selection: rgb(0x0064e1),
            on_selection: rgb(0xffffff),
            menu: rgb(0xf6f6f6),
            // NSColor.systemRed as Increase contrast draws it: the plain one read 3.0 to 1.
            warning: rgb(0xd70015),
            control: rgb(0xffffff),
            control_border: rgba(0x00000026),
            field: rgb(0xffffff),
            window: rgb(0xececec),
            accent: rgb(0x007aff),
            // Darker than NSColor.systemGreen (1.9 to 1 on a dialog, gaps row U22).
            success: rgb(0x1d7a34),
            on_accent: rgb(0xffffff),
        }
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

/// Windows' High Contrast, as its own programs draw under it and as the
/// egui window does: every background the window colour, every word and
/// line the text colour, what is chosen the highlight with its own text, a
/// button the button face, nothing see-through and no stripes. Nothing is
/// told by colour alone (a state has its words, a kind its picture's shape).
fn high_contrast(p: &dm_desktop_base::contrast::Palette) -> Theme {
    let c = |[r, g, b]: dm_desktop_base::contrast::Rgb| {
        rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
    };
    Theme {
        background: c(p.window),
        text: c(p.window_text),
        muted: c(p.window_text),
        stripe: rgba(0x00000000),
        separator: c(p.window_text),
        sidebar: c(p.window),
        selection: c(p.highlight),
        on_selection: c(p.highlight_text),
        menu: c(p.window),
        warning: c(p.window_text),
        control: c(p.button_face),
        control_border: c(p.button_text),
        field: c(p.window),
        window: c(p.window),
        accent: c(p.highlight),
        on_accent: c(p.highlight_text),
        success: c(p.window_text),
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
            // Darker than the GNOME palette's (3.9 to 1 on the side panel, gaps row U22).
            success: rgb(0x17733a),
        }
    }
}

/// A colour of the main window's frame (its title bar, toolbar, side panel
/// and status bar): clear under Mica, so the backdrop shows through it, as
/// Windows' own apps draw theirs; the colour itself everywhere else. The
/// list, dialogs and fields stay solid to be read on (gaps row G11).
pub fn frame(colour: Rgba) -> Rgba {
    if dm_desktop_base::mica::in_force() {
        gpui::transparent_black().into()
    } else {
        colour
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

    /// WCAG's contrast (`dm_desktop_base::contrast`) of `fore` blended onto
    /// `back`.
    fn contrast(fore: Rgba, back: Rgba) -> f32 {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let on = |f: f32, b: f32| byte(f * fore.a + b * (1.0 - fore.a));
        dm_desktop_base::contrast::ratio(
            [on(fore.r, back.r), on(fore.g, back.g), on(fore.b, back.b)],
            [byte(back.r), byte(back.g), byte(back.b)],
        )
    }

    #[test]
    fn every_systems_palette_keeps_its_words_readable() {
        let mut all = Vec::new();
        for dark in [false, true] {
            all.push(("Windows", dark, windows(dark)));
            all.push(("GNOME", dark, gnome(dark)));
            all.push(("macOS", dark, macos(dark)));
        }
        use dm_desktop_base::contrast::Palette;
        all.push(("High Contrast Black", true, high_contrast(&Palette::BLACK)));
        all.push(("High Contrast White", false, high_contrast(&Palette::WHITE)));
        let mut faint = Vec::new();
        for (system, dark, theme) in all {
            let at = format!("{system}, dark {dark}");
            assert!(contrast(theme.text, theme.background) >= 7.0, "{at}: text");
            assert!(contrast(theme.text, theme.sidebar) >= 7.0, "{at}: sidebar");
            // Muted and warning words at WCAG's 4.5 to 1 on every surface they are drawn on,
            // not 3 (gaps row U22): they are small (11 points) and say what matters.
            for (surface, back) in [
                ("list", theme.background),
                ("side panel", theme.sidebar),
                ("dialog", theme.window),
            ] {
                for (what, fore) in [
                    ("muted", theme.muted),
                    ("warning", theme.warning),
                    ("success", theme.success),
                ] {
                    let ratio = contrast(fore, back);
                    if ratio < 4.5 {
                        faint.push(format!("{at}: {what} on the {surface}: {ratio:.2}"));
                    }
                }
            }
            assert!(
                contrast(theme.on_selection, theme.selection) >= 3.0,
                "{at}: a chosen row"
            );
            assert!(
                contrast(theme.on_accent, theme.accent) >= 3.0,
                "{at}: default button"
            );
        }
        assert!(faint.is_empty(), "too faint: {faint:#?}");
        // A deep purple accent is taken by every look; a yellow one, which white words cannot
        // be read on, keeps dm's own (gaps row U33).
        let purple = dm_desktop_base::accent::from_one([0x88, 0x17, 0x98]);
        let yellow = dm_desktop_base::accent::from_one([0xff, 0xee, 0x00]);
        for dark in [false, true] {
            for own in [windows(dark), gnome(dark), macos(dark)] {
                let taken = own.with_accent(&purple, dark);
                if contrast(own.on_accent, taken.accent) >= 4.5 {
                    assert_ne!(taken.accent, own.accent, "dark {dark}");
                }
                let kept = own.with_accent(&yellow, false);
                assert!(kept.accent == own.accent || contrast(own.on_accent, kept.accent) >= 4.5);
            }
        }
        // No system's own colours pass for a chosen row's, which the list's pictures follow.
        for dark in [false, true] {
            for theme in [windows(dark), gnome(dark), macos(dark)] {
                assert!(!theme.is_chosen() && theme.chosen().is_chosen());
            }
        }
    }
}
