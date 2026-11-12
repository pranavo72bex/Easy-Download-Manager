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

    /// The colours for the system's appearance, as this system draws them.
    /// Under Windows' High Contrast, its own colours, whatever the look
    /// (contract B45: "on every look (light, dark, high contrast)").
    pub fn of(appearance: WindowAppearance) -> Theme {
        Theme::in_force(appearance, dm_desktop_base::contrast::system())
    }

    /// The colours for `appearance`, or High Contrast's `palette` where the
    /// system has one in force.
    fn in_force(
        appearance: WindowAppearance,
        palette: Option<dm_desktop_base::contrast::Palette>,
    ) -> Theme {
        if let Some(palette) = palette {
            return high_contrast(&palette);
        }
        let dark = matches!(
            appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        if cfg!(windows) {
            windows(dark)
        } else if cfg!(target_os = "macos") {
            macos(dark)
        } else {
            gnome(dark)
        }
    }

    /// The focus ring's colour round a control: the accent, and on the
    /// default button, which is filled with the accent, the words' colour
    /// on it (the accent there was a ring no one could see).
    pub fn ring(self, on_accent: bool) -> Rgba {
        if on_accent {
            self.on_accent
        } else {
            self.accent
        }
    }
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
        }
    } else {
        Theme {
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
            success: rgb(0x1b8553),
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
    cx.set_window_appearance(asked(choice));
}

/// The appearance `choice` asks for: none for the computer's own.
fn asked(choice: &str) -> Option<WindowAppearance> {
    match choice {
        "light" => Some(WindowAppearance::Light),
        "dark" => Some(WindowAppearance::Dark),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG's contrast (`dm_desktop_base::contrast`) of `fore` blended onto
    /// `back`.
    fn contrast(fore: Rgba, back: Rgba) -> f32 {
        dm_desktop_base::contrast::ratio(over(fore, back), over(back, back))
    }

    /// `fore` as it shows on `back`, in bytes: a see-through colour blended
    /// onto it.
    fn over(fore: Rgba, back: Rgba) -> [u8; 3] {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let on = |f: f32, b: f32| byte(f * fore.a + b * (1.0 - fore.a));
        [on(fore.r, back.r), on(fore.g, back.g), on(fore.b, back.b)]
    }

    /// Every system's palette, light and dark, named.
    fn systems() -> Vec<(String, Theme)> {
        let mut all = Vec::new();
        for dark in [false, true] {
            all.push((format!("Windows, dark {dark}"), windows(dark)));
            all.push((format!("GNOME, dark {dark}"), gnome(dark)));
            all.push((format!("macOS, dark {dark}"), macos(dark)));
        }
        all
    }

    /// The system's palettes and High Contrast's two.
    fn every_look() -> Vec<(String, Theme)> {
        use dm_desktop_base::contrast::Palette;
        let mut all = systems();
        all.push(("High Contrast Black".into(), high_contrast(&Palette::BLACK)));
        all.push(("High Contrast White".into(), high_contrast(&Palette::WHITE)));
        all
    }

    /// Options › General's look: Light or Dark whatever the computer's,
    /// else the computer's, followed as it changes.
    #[test]
    fn the_look_chosen_is_light_dark_or_the_computers() {
        assert_eq!(asked("light"), Some(WindowAppearance::Light));
        assert_eq!(asked("dark"), Some(WindowAppearance::Dark));
        for word in ["system", "", "classic", "anything"] {
            assert_eq!(asked(word), None, "{word} is the computer's");
        }
        let light = Theme::in_force(WindowAppearance::Light, None);
        let dark = Theme::in_force(WindowAppearance::Dark, None);
        assert_ne!(light.background, dark.background);
        assert_eq!(
            Theme::in_force(WindowAppearance::VibrantDark, None).background,
            dark.background
        );
        assert_eq!(
            Theme::in_force(WindowAppearance::VibrantLight, None).background,
            light.background
        );
    }

    /// Windows 11's palette is WinUI 3's brushes.
    #[test]
    fn the_windows_looks_are_winui_3s_brushes() {
        let white = rgb(0xffffff);
        let light = windows(false);
        assert_eq!(light.sidebar, rgb(0xf3f3f3), "SolidBackgroundFillColorBase");
        assert_eq!(light.accent, rgb(0x005fb8), "AccentFillColorDefault");
        assert_eq!(
            over(light.text, white),
            [0x1b, 0x1b, 0x1b],
            "TextFillColorPrimary"
        );
        let dark = windows(true);
        assert_eq!(dark.window, rgb(0x202020), "SolidBackgroundFillColorBase");
        assert_eq!(dark.accent, rgb(0x60cdff), "AccentFillColorDefault");
        assert_eq!(
            dark.on_accent,
            rgb(0x000000),
            "TextOnAccentFillColorPrimary"
        );
    }

    /// The focus ring and the selection can be seen in every look: the ring
    /// round the default button was the accent drawn on the accent.
    #[test]
    fn the_focus_ring_and_the_selection_can_be_seen_in_every_look() {
        for (look, t) in every_look() {
            let ring = contrast(t.ring(true), t.accent);
            assert!(ring >= 3.0, "{look}: ring on the default button {ring:.2}");
            for (name, surface) in [
                ("the list", t.background),
                ("a dialog", t.window),
                ("the side panel", t.sidebar),
            ] {
                let seen = contrast(t.ring(false), surface);
                assert!(seen >= 3.0, "{look}: ring on {name} {seen:.2}");
            }
            let words = contrast(t.on_selection, t.selection);
            assert!(words >= 3.0, "{look}: words on the selection {words:.2}");
        }
    }

    /// Hovering changes what a row, a place or a button looks like on every
    /// surface it sits on.
    #[test]
    fn hover_can_be_seen_on_every_surface() {
        for (look, t) in systems() {
            for surface in [t.background, t.sidebar, t.window] {
                assert_ne!(
                    over(t.stripe, surface),
                    over(surface, surface),
                    "{look}: a row's hover"
                );
            }
            // The title bar's buttons, on the frame.
            assert_ne!(
                over(t.control, t.sidebar),
                over(t.sidebar, t.sidebar),
                "{look}: a title bar button's hover"
            );
        }
    }

    /// Windows' High Contrast is the look whatever was chosen; a different
    /// scheme is a different look.
    #[test]
    fn a_system_high_contrast_theme_is_the_look_whatever_was_chosen() {
        use dm_desktop_base::contrast::Palette;
        let black = high_contrast(&Palette::BLACK);
        for appearance in [
            WindowAppearance::Light,
            WindowAppearance::Dark,
            WindowAppearance::VibrantLight,
            WindowAppearance::VibrantDark,
        ] {
            let t = Theme::in_force(appearance, Some(Palette::BLACK));
            assert_eq!(
                (t.background, t.text, t.selection, t.accent),
                (black.background, black.text, black.selection, black.accent),
                "{appearance:?}"
            );
        }
        let white = high_contrast(&Palette::WHITE);
        assert_ne!(black.background, white.background);
    }

    /// Every word and line on every surface it is drawn on, in each High
    /// Contrast scheme, reads at 4.5:1 at least, and nothing is see-through.
    #[test]
    fn the_high_contrast_looks_read_at_four_and_a_half_to_one() {
        use dm_desktop_base::contrast::Palette;
        for palette in [Palette::BLACK, Palette::WHITE] {
            let t = high_contrast(&palette);
            for (name, words, ground) in [
                ("text on the list", t.text, t.background),
                ("text on the side panel", t.text, t.sidebar),
                ("text on a dialog", t.text, t.window),
                ("text on a button", t.text, t.control),
                ("text in a field", t.text, t.field),
                ("text on a menu", t.text, t.menu),
                ("secondary words", t.muted, t.background),
                ("a selected row", t.on_selection, t.selection),
                ("words on the accent fill", t.on_accent, t.accent),
                ("a warning", t.warning, t.window),
                ("a line", t.separator, t.background),
                ("a button's edge", t.control_border, t.control),
            ] {
                let seen = contrast(words, ground);
                assert!(seen >= 4.5, "{name}: {seen:.1}:1 in {palette:?}");
            }
            for colour in [t.background, t.window, t.control, t.selection, t.accent] {
                assert_eq!(colour.a, 1.0, "nothing see-through in {palette:?}");
            }
        }
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
