//! Windows' High Contrast themes (Aquatic, Desert, Dusk, Night sky, and the
//! person's own): when one is on, the window is drawn in the system's own
//! colours, whatever look was chosen, as a Windows app is.
//!
//! Only Windows has them: [`system`] says `None` elsewhere. The colours are
//! read as the system names them (`COLOR_WINDOW`, `COLOR_WINDOWTEXT`, …) and
//! kept to be turned into a look by `Theme::HighContrast`; the system is
//! asked again at most once a second, so switching the theme in Settings
//! changes dm while it runs.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// An sRGB colour as its three bytes.
pub type Rgb = [u8; 3];

/// The system colours a High Contrast theme sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// `COLOR_WINDOW`: the background of the window and its dialogs.
    pub window: Rgb,
    /// `COLOR_WINDOWTEXT`: text on it, and its lines.
    pub window_text: Rgb,
    /// `COLOR_BTNFACE`: a button's face.
    pub button_face: Rgb,
    /// `COLOR_BTNTEXT`: words on a button.
    pub button_text: Rgb,
    /// `COLOR_HIGHLIGHT`: what is selected or pressed.
    pub highlight: Rgb,
    /// `COLOR_HIGHLIGHTTEXT`: words on it.
    pub highlight_text: Rgb,
    /// `COLOR_GRAYTEXT`: what is disabled.
    pub gray_text: Rgb,
    /// `COLOR_HOTLIGHT`: a link.
    pub hotlight: Rgb,
}

impl Palette {
    /// The "High Contrast Black" theme's colours, for where the system's are
    /// not to be had.
    pub const BLACK: Palette = Palette {
        window: [0x00, 0x00, 0x00],
        window_text: [0xff, 0xff, 0xff],
        button_face: [0x00, 0x00, 0x00],
        button_text: [0xff, 0xff, 0xff],
        highlight: [0x1a, 0xeb, 0xff],
        highlight_text: [0x00, 0x00, 0x00],
        gray_text: [0x3f, 0xf2, 0x3f],
        hotlight: [0xff, 0xff, 0x00],
    };

    /// "High Contrast White".
    pub const WHITE: Palette = Palette {
        window: [0xff, 0xff, 0xff],
        window_text: [0x00, 0x00, 0x00],
        button_face: [0xff, 0xff, 0xff],
        button_text: [0x00, 0x00, 0x00],
        highlight: [0x37, 0x00, 0x6e],
        highlight_text: [0xff, 0xff, 0xff],
        gray_text: [0x60, 0x00, 0x00],
        hotlight: [0x00, 0x00, 0x9e],
    };

    /// Whether the window colour is dark.
    pub fn is_dark(&self) -> bool {
        luminance(self.window) < 0.5
    }
}

/// The relative luminance of a colour, 0 to 1.
pub fn luminance([r, g, b]: Rgb) -> f32 {
    let linear = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

/// The contrast ratio of two colours, 1 to 21.
pub fn ratio(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// A system colour as bytes: a `COLORREF` is `0x00BBGGRR`.
pub fn from_colorref(colorref: u32) -> Rgb {
    [
        (colorref & 0xff) as u8,
        ((colorref >> 8) & 0xff) as u8,
        ((colorref >> 16) & 0xff) as u8,
    ]
}

/// The palette High Contrast is in force with, if it is: asked of the system
/// where there is one.
#[cfg(windows)]
fn ask() -> Option<Palette> {
    use windows_sys::Win32::Graphics::Gdi::{
        GetSysColor, COLOR_BTNFACE, COLOR_BTNTEXT, COLOR_GRAYTEXT, COLOR_HIGHLIGHT,
        COLOR_HIGHLIGHTTEXT, COLOR_HOTLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT,
    };
    use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETHIGHCONTRAST};

    let mut info = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        dwFlags: 0,
        lpszDefaultScheme: std::ptr::null_mut(),
    };
    // SAFETY: `info` is a local the size of which is named in its first
    // field, which is what the call needs; nothing is kept from it.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            info.cbSize,
            (&mut info as *mut HIGHCONTRASTW).cast(),
            0,
        )
    };
    if read == 0 || info.dwFlags & HCF_HIGHCONTRASTON == 0 {
        return None;
    }
    // SAFETY: `GetSysColor` takes an index and returns a value.
    let color = |index| from_colorref(unsafe { GetSysColor(index) });
    Some(Palette {
        window: color(COLOR_WINDOW),
        window_text: color(COLOR_WINDOWTEXT),
        button_face: color(COLOR_BTNFACE),
        button_text: color(COLOR_BTNTEXT),
        highlight: color(COLOR_HIGHLIGHT),
        highlight_text: color(COLOR_HIGHLIGHTTEXT),
        gray_text: color(COLOR_GRAYTEXT),
        hotlight: color(COLOR_HOTLIGHT),
    })
}

/// Nothing says so here. A debug build can be shown the look all the same,
/// with `DM_HIGH_CONTRAST=black` or `white`, to see it where there is no
/// Windows.
#[cfg(not(windows))]
fn ask() -> Option<Palette> {
    if !cfg!(debug_assertions) {
        return None;
    }
    match std::env::var("DM_HIGH_CONTRAST").ok()?.as_str() {
        "black" => Some(Palette::BLACK),
        "white" => Some(Palette::WHITE),
        _ => None,
    }
}

/// What the system said last, and when.
static LAST: Mutex<Option<(Instant, Option<Palette>)>> = Mutex::new(None);

/// The High Contrast palette in force, or `None`. Asked of the system at
/// most once a second.
pub fn system() -> Option<Palette> {
    let mut last = LAST.lock().ok()?;
    if let Some((at, found)) = *last {
        if at.elapsed() < Duration::from_secs(1) {
            return found;
        }
    }
    let found = ask();
    *last = Some((Instant::now(), found));
    found
}

/// The palette the High Contrast look is built from: the system's, or the
/// black theme's where a test or a stale setting asks for the look with none.
pub fn current() -> Palette {
    system().unwrap_or(Palette::BLACK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colorref_is_blue_green_red() {
        assert_eq!(from_colorref(0x00_33_22_11), [0x11, 0x22, 0x33]);
    }

    #[test]
    fn the_two_built_in_palettes_hold_their_own_pairs_apart() {
        for palette in [Palette::BLACK, Palette::WHITE] {
            assert!(ratio(palette.window, palette.window_text) >= 7.0);
            assert!(ratio(palette.highlight, palette.highlight_text) >= 7.0);
            assert!(ratio(palette.window, palette.hotlight) >= 4.5);
        }
        assert!(Palette::BLACK.is_dark());
        assert!(!Palette::WHITE.is_dark());
    }

    #[test]
    fn off_windows_there_is_no_high_contrast_unless_a_debug_build_is_asked_to_show_it() {
        if cfg!(not(windows)) && std::env::var_os("DM_HIGH_CONTRAST").is_none() {
            assert_eq!(system(), None);
        }
    }
}
