// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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

/// `fore` blended onto `back`, as a colour of its own (a stripe over the list).
fn over_rgba(fore: Rgba, back: Rgba) -> Rgba {
    let [r, g, b] = over(fore, back);
    rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
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
        // A chosen row's words and the default button's are words like any other: 4.5 to 1
        // (the system blue under white read 4.0 on macOS and 3.8 on GNOME).
        assert!(
            contrast(theme.on_selection, theme.selection) >= 4.5,
            "{at}: a chosen row"
        );
        assert!(
            contrast(theme.on_accent, theme.accent) >= 4.5,
            "{at}: default button"
        );
        // Words and marks in the accent (a running download's state, "+N more", a link, a
        // bar) on every surface they are drawn on, a striped row's included.
        for (surface, back) in [
            ("list", theme.background),
            ("striped row", over_rgba(theme.stripe, theme.background)),
            ("side panel", theme.sidebar),
            ("dialog", theme.window),
            ("menu", theme.menu),
        ] {
            let ratio = contrast(theme.accent_text, back);
            if ratio < 4.5 {
                faint.push(format!("{at}: accent words on the {surface}: {ratio:.2}"));
            }
        }
    }
    assert!(faint.is_empty(), "too faint: {faint:#?}");
    // A deep purple accent is taken by every look; a yellow one, which white words cannot
    // be read on, keeps dm's own (gaps row U33).
    let purple = dm_desktop_base::accent::from_one([0x88, 0x17, 0x98]);
    let yellow = dm_desktop_base::accent::from_one([0xff, 0xee, 0x00]);
    for dark in [false, true] {
        for own in [windows(dark), gnome(dark), macos(dark)] {
            let taken = own.with_accent(&purple, dark);
            let [r, g, b] = if dark { purple.light1 } else { purple.dark1 };
            let shade = rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b));
            if contrast(own.on_accent, shade) >= 4.5 && contrast(own.on_selection, shade) >= 4.5 {
                assert_eq!(taken.accent, shade, "dark {dark}");
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

/// A source list's chosen entry (the side panel's place, Options' page) keeps its words
/// readable on its soft fill: all of them, its count and its line under the name too, drawn in
/// the text's colour at 4.5 to 1 (the secondary colour read 4.29 there on GNOME's light side
/// panel), and the accent's picture and bar at 3, on the side panel and on a dialog.
#[test]
fn the_soft_choice_keeps_its_words_readable() {
    for (at, theme) in every_look() {
        for (surface, back) in [
            ("side panel", theme.sidebar),
            ("dialog", theme.window),
            ("list", theme.background),
            ("striped row", over_rgba(theme.stripe, theme.background)),
        ] {
            let chosen = over_rgba(theme.chosen_soft(), back);
            let name = contrast(theme.text, chosen);
            let mark = contrast(theme.accent_text, chosen);
            assert!(name >= 4.5, "{at}: a name on the {surface}: {name:.2}");
            assert!(mark >= 3.0, "{at}: a picture on the {surface}: {mark:.2}");
        }
    }
}
