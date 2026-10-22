//! The computer's accent colour, for the modern look's accent brushes: the
//! primary button, the selection indicator, a checked box, the progress bar
//! and words in the accent, as Fluent draws them from the system's accent.
//!
//! Windows keeps the whole palette Fluent's brushes are taken from (three
//! lighter shades, the accent, three darker); macOS and Linux give one
//! colour, and the shades are made from it. Read on a thread of its own
//! every few seconds, as the system theme is. An accent that would leave
//! text on it, or words in it, hard to read keeps dm's own.

use std::sync::{Mutex, Once};
use std::time::Duration;

use egui::Color32;

use crate::theme::Tokens;

/// The accent and the shades the accent brushes are drawn from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub light3: Color32,
    pub light2: Color32,
    pub light1: Color32,
    pub base: Color32,
    pub dark1: Color32,
    pub dark2: Color32,
    pub dark3: Color32,
}

static FOUND: Mutex<Option<Palette>> = Mutex::new(None);

/// The computer's accent, where it says one.
pub fn system() -> Option<Palette> {
    static WATCH: Once = Once::new();
    WATCH.call_once(|| {
        *dm_core::lock(&FOUND) = ask();
        let _ = std::thread::Builder::new()
            .name("dm-accent".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_secs(5));
                *dm_core::lock(&FOUND) = ask();
            });
    });
    *dm_core::lock(&FOUND)
}

#[cfg(windows)]
fn ask() -> Option<Palette> {
    let bytes = crate::autostart::registry::binary(
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent",
        "AccentPalette",
    )?;
    from_windows(&bytes)
}

#[cfg(target_os = "macos")]
fn ask() -> Option<Palette> {
    use objc2_app_kit::{NSColor, NSColorSpace};
    let accent = NSColor::controlAccentColor();
    let srgb = accent.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    from_unit(
        srgb.redComponent(),
        srgb.greenComponent(),
        srgb.blueComponent(),
    )
}

#[cfg(not(any(windows, target_os = "macos")))]
fn ask() -> Option<Palette> {
    let reply = crate::system_theme::run(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.ReadOne",
            "org.freedesktop.appearance",
            "accent-color",
        ],
    )?;
    from_portal(&reply)
}

/// Windows' AccentPalette: eight colours of four bytes (red, green, blue,
/// alpha), lightest first, the accent fourth.
#[cfg_attr(not(windows), allow(dead_code))]
fn from_windows(bytes: &[u8]) -> Option<Palette> {
    let colour = |n: usize| {
        let c = bytes.get(n * 4..n * 4 + 3)?;
        Some(Color32::from_rgb(c[0], c[1], c[2]))
    };
    Some(Palette {
        light3: colour(0)?,
        light2: colour(1)?,
        light1: colour(2)?,
        base: colour(3)?,
        dark1: colour(4)?,
        dark2: colour(5)?,
        dark3: colour(6)?,
    })
}

/// The desktop portal's `accent-color`, `(<(0.21, 0.52, 0.89)>,)`: three
/// parts from 0 to 1. One out of that range means none was chosen.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn from_portal(reply: &str) -> Option<Palette> {
    let inner = reply.rsplit('(').next()?.split(')').next()?;
    let parts: Vec<f64> = inner
        .split(',')
        .filter_map(|p| p.trim().parse::<f64>().ok())
        .collect();
    match parts[..] {
        [r, g, b] => from_unit(r, g, b),
        _ => None,
    }
}

/// One colour given as parts from 0 to 1.
#[cfg_attr(windows, allow(dead_code))]
fn from_unit(r: f64, g: f64, b: f64) -> Option<Palette> {
    let ok = |v: f64| (0.0..=1.0).contains(&v);
    if !(ok(r) && ok(g) && ok(b)) {
        return None;
    }
    let byte = |v: f64| (v * 255.0).round() as u8;
    Some(from_one(Color32::from_rgb(byte(r), byte(g), byte(b))))
}

/// The shades of one accent, made as Windows makes them near enough: its
/// hue and saturation kept, its lightness stepped towards black and white.
/// For Windows' default blue this gives its own palette within a few
/// steps of each channel.
pub fn from_one(accent: Color32) -> Palette {
    let (h, s, l) = hsl(accent);
    let darker = |f: f32| rgb(h, s, l * f);
    let lighter = |f: f32| rgb(h, s, l + (1.0 - l) * f);
    Palette {
        light3: lighter(0.66),
        light2: lighter(0.465),
        light1: lighter(0.25),
        base: accent,
        dark1: darker(0.87),
        dark2: darker(0.69),
        dark3: darker(0.5),
    }
}

/// `t` with its accent brushes from `palette`, as Fluent takes them: in
/// the light look the darker shades, in the dark look the lighter. The
/// classic look keeps its own; so does any look where the palette would
/// leave text unreadable.
pub fn tokens(t: Tokens, palette: &Palette) -> Tokens {
    if !t.modern {
        return t;
    }
    let (brand, words) = if t.dark {
        (palette.light2, palette.light3)
    } else {
        (palette.dark1, palette.dark2)
    };
    let over = |c: Color32, share: f32| mix(c, t.dialog, share);
    let with = Tokens {
        accent: words,
        brand,
        brand_hover: over(brand, 0.9),
        brand_pressed: over(brand, 0.8),
        text_selection: palette.base,
        ..t
    };
    let readable = crate::theme::contrast(with.on_brand, with.brand) >= 4.5
        && crate::theme::contrast(with.accent, with.list) >= 4.5
        && crate::theme::contrast(with.accent, with.dialog) >= 4.5;
    if readable {
        with
    } else {
        t
    }
}

/// `a` laid over `b`, `share` of it showing.
fn mix(a: Color32, b: Color32, share: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 * share + y as f32 * (1.0 - share)).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

fn hsl(c: Color32) -> (f32, f32, f32) {
    let (r, g, b) = (
        c.r() as f32 / 255.0,
        c.g() as f32 / 255.0,
        c.b() as f32 / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

fn rgb(h: f32, s: f32, l: f32) -> Color32 {
    let l = l.clamp(0.0, 1.0);
    if s == 0.0 {
        let v = (l * 255.0).round() as u8;
        return Color32::from_rgb(v, v, v);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    Color32::from_rgb(channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    fn near(a: Color32, b: Color32, by: u8) -> bool {
        a.r().abs_diff(b.r()) <= by && a.g().abs_diff(b.g()) <= by && a.b().abs_diff(b.b()) <= by
    }

    #[test]
    fn windows_default_blue_makes_the_brushes_dm_already_has() {
        let palette = from_one(Color32::from_rgb(0x00, 0x78, 0xd4));
        let light = tokens(Theme::Light.tokens(), &palette);
        let dark = tokens(Theme::Dark.tokens(), &palette);
        let own_light = Theme::Light.tokens();
        let own_dark = Theme::Dark.tokens();
        // Windows also turns its darker shades a little towards blue, which
        // stepping the lightness alone does not; on Windows its own palette
        // is read rather than made.
        assert!(near(light.brand, own_light.brand, 24), "{:?}", light.brand);
        assert!(
            near(light.accent, own_light.accent, 24),
            "{:?}",
            light.accent
        );
        assert!(near(dark.brand, own_dark.brand, 24), "{:?}", dark.brand);
        assert!(near(dark.accent, own_dark.accent, 24), "{:?}", dark.accent);
    }

    #[test]
    fn windows_palette_is_read_lightest_first() {
        let mut bytes = Vec::new();
        for n in 0..8u8 {
            bytes.extend([n, n, n, 0xff]);
        }
        let palette = from_windows(&bytes).unwrap();
        assert_eq!(palette.light3, Color32::from_rgb(0, 0, 0));
        assert_eq!(palette.base, Color32::from_rgb(3, 3, 3));
        assert_eq!(palette.dark3, Color32::from_rgb(6, 6, 6));
        assert_eq!(from_windows(&bytes[..20]), None);
    }

    #[test]
    fn the_portal_says_a_colour_or_none_chosen() {
        let chosen = from_portal("(<(0.2078, 0.5176, 0.8941)>,)").unwrap();
        assert_eq!(chosen.base, Color32::from_rgb(53, 132, 228));
        assert_eq!(from_portal("(<(-1.0, -1.0, -1.0)>,)"), None);
        assert_eq!(from_portal("nonsense"), None);
    }

    #[test]
    fn an_accent_that_would_be_unreadable_keeps_dms_own() {
        // Yellow: white words on it, and yellow words on white, fail.
        let yellow = from_one(Color32::from_rgb(0xff, 0xee, 0x00));
        let light = Theme::Light.tokens();
        assert_eq!(tokens(light, &yellow), light);
        // The classic look keeps its own whatever the accent.
        let classic = Theme::Classic.tokens();
        let purple = from_one(Color32::from_rgb(0x88, 0x17, 0x98));
        assert_eq!(tokens(classic, &purple), classic);
        // A deep purple reads well in both modern looks.
        assert_ne!(tokens(light, &purple), light);
        assert_ne!(tokens(Theme::Dark.tokens(), &purple), Theme::Dark.tokens());
    }
}
