// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The computer's accent colour, for every window: Windows keeps the whole palette its own
//! programs take their accent brushes from (three lighter shades, the accent, three darker);
//! macOS and Linux give one colour, and the shades are made from it. Read on a thread of its
//! own every few seconds, as the system theme is. Moved here from the egui window (gaps row
//! U33), as plain red, green and blue.

use std::sync::{Mutex, Once};
use std::time::Duration;

use crate::contrast::Rgb;

/// The accent and the shades the accent brushes are drawn from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// The palest tint, three steps lighter than `base`.
    pub light3: Rgb,
    /// A tint two steps lighter than `base`.
    pub light2: Rgb,
    /// A tint one step lighter than `base`.
    pub light1: Rgb,
    /// The accent itself.
    pub base: Rgb,
    /// A shade one step darker than `base`.
    pub dark1: Rgb,
    /// A shade two steps darker than `base`.
    pub dark2: Rgb,
    /// The darkest shade, three steps darker than `base`.
    pub dark3: Rgb,
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
        Some([c[0], c[1], c[2]])
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
    Some(from_one([byte(r), byte(g), byte(b)]))
}

/// The shades of one accent, made as Windows makes them near enough: its
/// hue and saturation kept, its lightness stepped towards black and white.
/// For Windows' default blue this gives its own palette within a few
/// steps of each channel.
pub fn from_one(accent: Rgb) -> Palette {
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

fn hsl(c: Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (
        c[0] as f32 / 255.0,
        c[1] as f32 / 255.0,
        c[2] as f32 / 255.0,
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

fn rgb(h: f32, s: f32, l: f32) -> Rgb {
    let l = l.clamp(0.0, 1.0);
    if s == 0.0 {
        let v = (l * 255.0).round() as u8;
        return [v, v, v];
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
    [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_palette_is_read_lightest_first() {
        let mut bytes = Vec::new();
        for n in 0..8u8 {
            bytes.extend([n, n, n, 0xff]);
        }
        let palette = from_windows(&bytes).unwrap();
        assert_eq!(palette.light3, [0, 0, 0]);
        assert_eq!(palette.base, [3, 3, 3]);
        assert_eq!(palette.dark3, [6, 6, 6]);
        assert_eq!(from_windows(&bytes[..20]), None);
    }

    #[test]
    fn the_portal_says_a_colour_or_none_chosen() {
        let chosen = from_portal("(<(0.2078, 0.5176, 0.8941)>,)").unwrap();
        assert_eq!(chosen.base, [53, 132, 228]);
        assert_eq!(from_portal("(<(-1.0, -1.0, -1.0)>,)"), None);
        assert_eq!(from_portal("nonsense"), None);
    }

    /// The shades keep the accent's hue, lighter and darker in step.
    #[test]
    fn one_accent_makes_lighter_and_darker_shades() {
        let palette = from_one([0x00, 0x78, 0xd4]);
        let light = |c: Rgb| u32::from(c[0]) + u32::from(c[1]) + u32::from(c[2]);
        assert!(light(palette.light3) > light(palette.light1));
        assert!(light(palette.light1) > light(palette.base));
        assert!(light(palette.base) > light(palette.dark1));
        assert!(light(palette.dark1) > light(palette.dark3));
    }
}
