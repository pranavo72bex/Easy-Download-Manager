//! Whether the desktop prefers dark, where the window library cannot say.
//!
//! On macOS and Windows the window is told the system's appearance. On
//! Linux it is not, so "As the computer is" read as light on every dark
//! desktop. Linux desktops publish the choice in a few places, asked in this
//! order, the first that answers deciding:
//!
//! 1. the freedesktop settings portal's `color-scheme` (GNOME, KDE, and
//!    others through xdg-desktop-portal): 1 is dark, 2 light, 0 no choice;
//! 2. GNOME's own `color-scheme` setting (`prefer-dark`, `prefer-light`);
//! 3. the GTK theme's name, which a dark one says (`Adwaita-dark`);
//! 4. `GTK_THEME` in the environment (`Adwaita:dark`);
//! 5. KDE's colour scheme, in `~/.config/kdeglobals`.
//!
//! Read once when first asked and again every few seconds on a thread of
//! its own, so switching the desktop to dark turns dm dark too, and the
//! window never waits on a program it runs.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Once;
use std::time::Duration;

const UNKNOWN: u8 = 0;
const LIGHT: u8 = 1;
const DARK: u8 = 2;

static FOUND: AtomicU8 = AtomicU8::new(UNKNOWN);

/// Whether the desktop prefers dark: `None` where it does not say (or
/// off Linux, where the window library answers instead).
pub fn prefers_dark() -> Option<bool> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    static WATCH: Once = Once::new();
    WATCH.call_once(|| {
        FOUND.store(encode(ask()), Ordering::Relaxed);
        let _ = std::thread::Builder::new()
            .name("dm-system-theme".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_secs(5));
                FOUND.store(encode(ask()), Ordering::Relaxed);
            });
    });
    match FOUND.load(Ordering::Relaxed) {
        DARK => Some(true),
        LIGHT => Some(false),
        _ => None,
    }
}

fn encode(dark: Option<bool>) -> u8 {
    match dark {
        Some(true) => DARK,
        Some(false) => LIGHT,
        None => UNKNOWN,
    }
}

/// The desktop's answer, asked in order.
fn ask() -> Option<bool> {
    let portal = |method: &str| {
        run(
            "gdbus",
            &[
                "call",
                "--session",
                "--dest",
                "org.freedesktop.portal.Desktop",
                "--object-path",
                "/org/freedesktop/portal/desktop",
                "--method",
                method,
                "org.freedesktop.appearance",
                "color-scheme",
            ],
        )
    };
    portal("org.freedesktop.portal.Settings.ReadOne")
        .and_then(|out| from_portal(&out))
        .or_else(|| {
            portal("org.freedesktop.portal.Settings.Read").and_then(|out| from_portal(&out))
        })
        .or_else(|| {
            run(
                "gsettings",
                &["get", "org.gnome.desktop.interface", "color-scheme"],
            )
            .and_then(|out| from_gnome_scheme(&out))
        })
        .or_else(|| {
            run(
                "gsettings",
                &["get", "org.gnome.desktop.interface", "gtk-theme"],
            )
            .and_then(|out| from_theme_name(&out))
        })
        .or_else(|| {
            std::env::var("GTK_THEME")
                .ok()
                .and_then(|name| from_theme_name(&name))
        })
        .or_else(|| {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .filter(|v| !v.is_empty())
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
                })?;
            let kde = config.join("kdeglobals");
            from_kdeglobals(&std::fs::read_to_string(kde).ok()?)
        })
}

/// A program's output, if it ran and finished within a second.
pub(crate) fn run(program: &str, args: &[&str]) -> Option<String> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .ok()?;
    for _ in 0..100 {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let mut out = String::new();
                std::io::Read::read_to_string(child.stdout.as_mut()?, &mut out).ok()?;
                return Some(out);
            }
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

/// The portal's reply, `(<uint32 1>,)` from ReadOne or `(<<uint32 1>>,)`
/// from Read: 1 prefers dark, 2 light, 0 says nothing.
fn from_portal(reply: &str) -> Option<bool> {
    let number = reply.rsplit("uint32").next()?;
    let value: u32 = number
        .trim()
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()?;
    match value {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// GNOME's `color-scheme`: `'prefer-dark'`, `'prefer-light'`, or
/// `'default'`, which says nothing.
fn from_gnome_scheme(value: &str) -> Option<bool> {
    match value.trim().trim_matches('\'') {
        "prefer-dark" => Some(true),
        "prefer-light" => Some(false),
        _ => None,
    }
}

/// A theme's name says dark where it is (`'Adwaita-dark'`, `Yaru:dark`); a
/// name without it is no answer, since most light themes do not say light.
fn from_theme_name(name: &str) -> Option<bool> {
    name.to_ascii_lowercase().contains("dark").then_some(true)
}

/// KDE's colour scheme, in `[General]`: `ColorScheme=BreezeDark`.
fn from_kdeglobals(text: &str) -> Option<bool> {
    let mut general = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            general = line == "[General]";
        } else if general {
            if let Some(scheme) = line.strip_prefix("ColorScheme=") {
                return Some(scheme.to_ascii_lowercase().contains("dark"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_portal_says_dark_light_or_nothing() {
        assert_eq!(from_portal("(<uint32 1>,)\n"), Some(true));
        assert_eq!(from_portal("(<<uint32 1>>,)"), Some(true));
        assert_eq!(from_portal("(<<uint32 2>>,)"), Some(false));
        assert_eq!(from_portal("(<<uint32 0>>,)"), None, "no preference");
        assert_eq!(from_portal(""), None);
        assert_eq!(from_portal("Error: No such interface"), None);
    }

    #[test]
    fn gnome_says_prefer_dark_or_light() {
        assert_eq!(from_gnome_scheme("'prefer-dark'\n"), Some(true));
        assert_eq!(from_gnome_scheme("'prefer-light'"), Some(false));
        assert_eq!(from_gnome_scheme("'default'"), None);
    }

    #[test]
    fn a_dark_theme_says_so_in_its_name() {
        assert_eq!(from_theme_name("'Adwaita-dark'"), Some(true));
        assert_eq!(from_theme_name("Yaru:dark"), Some(true));
        assert_eq!(
            from_theme_name("'Adwaita'"),
            None,
            "light themes do not say"
        );
    }

    #[test]
    fn kde_says_it_in_its_colour_scheme() {
        let dark = "[KDE]\nColorScheme=Ignored\n[General]\nColorScheme=BreezeDark\n";
        assert_eq!(from_kdeglobals(dark), Some(true));
        assert_eq!(
            from_kdeglobals("[General]\nColorScheme=BreezeLight\n"),
            Some(false)
        );
        assert_eq!(from_kdeglobals("[General]\nName=x\n"), None);
    }

    #[test]
    fn a_program_that_is_not_there_is_no_answer() {
        assert_eq!(run("dm-no-such-program", &[]), None);
    }
}
