//! Whether the computer asks for less motion: Windows' "Animation effects"
//! off, macOS's Reduce motion, GNOME's animations off. Where it does, the
//! window does not animate: menus, collapsing sections and scrolling to a
//! row arrive at once.
//!
//! Read on a thread of its own every few seconds, as the system theme is,
//! so the window's thread never waits on the system and a change is taken
//! up without a restart.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Once;
use std::time::Duration;

const UNKNOWN: u8 = 0;
const REDUCED: u8 = 1;
const FULL: u8 = 2;

static FOUND: AtomicU8 = AtomicU8::new(UNKNOWN);

/// Whether motion should be left out. Not knowing is motion as usual.
pub fn reduced() -> bool {
    static WATCH: Once = Once::new();
    WATCH.call_once(|| {
        FOUND.store(encode(ask()), Ordering::Relaxed);
        let _ = std::thread::Builder::new()
            .name("dm-motion".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_secs(5));
                FOUND.store(encode(ask()), Ordering::Relaxed);
            });
    });
    FOUND.load(Ordering::Relaxed) == REDUCED
}

fn encode(reduced: Option<bool>) -> u8 {
    match reduced {
        Some(true) => REDUCED,
        Some(false) => FULL,
        None => UNKNOWN,
    }
}

#[cfg(target_os = "windows")]
fn ask() -> Option<bool> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
    };
    let mut on: windows_sys::core::BOOL = 1;
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the pointer,
    // which points at `on`, alive for the call.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut on as *mut windows_sys::core::BOOL).cast(),
            0,
        )
    };
    (ok != 0).then_some(on == 0)
}

#[cfg(target_os = "macos")]
fn ask() -> Option<bool> {
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    Some(workspace.accessibilityDisplayShouldReduceMotion())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn ask() -> Option<bool> {
    crate::system_theme::run(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "enable-animations"],
    )
    .and_then(|out| from_gnome(&out))
}

/// GNOME's `enable-animations`: `false` asks for less motion.
#[cfg_attr(any(target_os = "windows", target_os = "macos"), allow(dead_code))]
fn from_gnome(value: &str) -> Option<bool> {
    match value.trim() {
        "false" => Some(true),
        "true" => Some(false),
        _ => None,
    }
}

/// The window's motion as the system asks: none at all, or egui's own.
pub fn apply(ctx: &egui::Context, reduced: bool) {
    ctx.all_styles_mut(|style| {
        let usual = egui::Style::default();
        if reduced {
            style.animation_time = 0.0;
            style.scroll_animation = egui::style::ScrollAnimation::none();
        } else {
            style.animation_time = usual.animation_time;
            style.scroll_animation = usual.scroll_animation;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_says_whether_it_animates() {
        assert_eq!(from_gnome("false\n"), Some(true));
        assert_eq!(from_gnome("true"), Some(false));
        assert_eq!(from_gnome(""), None);
    }

    #[test]
    fn less_motion_means_none_and_back_means_egui_own() {
        let ctx = egui::Context::default();
        apply(&ctx, true);
        assert_eq!(ctx.style_of(egui::Theme::Dark).animation_time, 0.0);
        apply(&ctx, false);
        assert_eq!(
            ctx.style_of(egui::Theme::Dark).animation_time,
            egui::Style::default().animation_time
        );
    }
}
