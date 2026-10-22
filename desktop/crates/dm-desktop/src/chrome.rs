//! The window's own frame on Windows: the title bar dark where the look is
//! dark, and in the colour of the window's background, so title bar and
//! content read as one surface, as Fluent's windows do. Elsewhere the
//! system draws its own and nothing is asked of it.
//!
//! Mica, opt-in (Options › General, Windows 11 22H2 and later, from dm's
//! next start): the desktop's colours through the window's frame. It needs
//! a see-through window, which an old GPU driver can turn black, so it is
//! never on by default (docs/decisions.md).

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether this run's window was made see-through for Mica.
static MICA: AtomicBool = AtomicBool::new(false);

/// Whether this run's window has Mica behind it.
pub fn mica() -> bool {
    MICA.load(Ordering::Relaxed)
}

/// The first Windows build with `DWMWA_SYSTEMBACKDROP_TYPE`: 11 22H2.
pub const MICA_BUILD: u32 = 22621;

/// Whether this computer can draw Mica: Windows 11 22H2 or later.
pub fn mica_possible() -> bool {
    #[cfg(windows)]
    {
        crate::autostart::registry::machine_string(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "CurrentBuildNumber",
        )
        .and_then(|b| b.trim().parse::<u32>().ok())
        .is_some_and(|build| build >= MICA_BUILD)
    }
    #[cfg(not(windows))]
    false
}

/// Puts Mica in force for this run where it is `wanted` and possible, and
/// says whether it is: the window is then to be made see-through, and its
/// frame drawn clear.
pub fn use_mica(wanted: bool) -> bool {
    let on = wanted && mica_possible();
    MICA.store(on, Ordering::Relaxed);
    on
}

/// A colour as Windows takes it: `0x00BBGGRR`.
pub fn colorref(colour: egui::Color32) -> u32 {
    u32::from(colour.r()) | u32::from(colour.g()) << 8 | u32::from(colour.b()) << 16
}

/// Tells the window's frame the look: dark or light, and the colours of
/// its caption and its caption's words. Windows 10 before 1809 and other
/// systems ignore it.
pub fn title_bar(
    window: raw_window_handle::RawWindowHandle,
    dark: bool,
    caption: egui::Color32,
    text: egui::Color32,
) {
    #[cfg(windows)]
    if let raw_window_handle::RawWindowHandle::Win32(handle) = window {
        use windows_sys::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT, DWMWA_TEXT_COLOR,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
        };
        let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        let set = |attribute: i32, value: u32| {
            // SAFETY: each attribute takes one 32-bit value, read from a
            // local alive for the call; an attribute this Windows does not
            // know is refused with an error and changes nothing.
            unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    attribute as u32,
                    (&value as *const u32).cast(),
                    std::mem::size_of::<u32>() as u32,
                );
            }
        };
        set(DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(dark));
        if mica() {
            use windows_sys::Win32::Graphics::Dwm::{
                DwmExtendFrameIntoClientArea, DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
            };
            use windows_sys::Win32::UI::Controls::MARGINS;
            // The system's caption over Mica, not a colour of dm's.
            set(DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT);
            set(DWMWA_TEXT_COLOR, DWMWA_COLOR_DEFAULT);
            set(DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_MAINWINDOW as u32);
            let whole = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            // SAFETY: the margins are a local alive for the call.
            unsafe {
                DwmExtendFrameIntoClientArea(hwnd, &whole);
            }
        } else {
            set(DWMWA_CAPTION_COLOR, colorref(caption));
            set(DWMWA_TEXT_COLOR, colorref(text));
        }
    }
    let _ = (window, dark, caption, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mica_is_never_in_force_where_it_cannot_be_drawn() {
        if !mica_possible() {
            assert!(!use_mica(true));
            assert!(!mica());
        }
        assert!(!use_mica(false), "never without being asked for");
    }

    #[test]
    fn a_colour_is_written_blue_green_red() {
        assert_eq!(
            colorref(egui::Color32::from_rgb(0x12, 0x34, 0x56)),
            0x0056_3412
        );
        assert_eq!(colorref(egui::Color32::WHITE), 0x00ff_ffff);
    }
}
