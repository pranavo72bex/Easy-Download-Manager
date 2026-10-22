//! The window's own frame on Windows: the title bar dark where the look is
//! dark, and in the colour of the window's background, so title bar and
//! content read as one surface, as Fluent's windows do. Elsewhere the
//! system draws its own and nothing is asked of it.

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
            DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
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
        set(DWMWA_CAPTION_COLOR, colorref(caption));
        set(DWMWA_TEXT_COLOR, colorref(text));
    }
    let _ = (window, dark, caption, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_is_written_blue_green_red() {
        assert_eq!(
            colorref(egui::Color32::from_rgb(0x12, 0x34, 0x56)),
            0x0056_3412
        );
        assert_eq!(colorref(egui::Color32::WHITE), 0x00ff_ffff);
    }
}
