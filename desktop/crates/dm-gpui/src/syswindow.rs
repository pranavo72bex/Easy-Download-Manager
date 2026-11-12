// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm asks of a system window that GPUI has no way to ask: put it away
//! without closing it (into the tray) and bring it back, move it (the
//! control port's `ui.move_window`, as a drag by its title bar would), and
//! picture it as it stands on screen (`ui.screenshot`). Each system its own
//! way: macOS through AppKit, Windows through Win32 on the window's `HWND`,
//! Linux on X11 through the X server itself (a connection of dm's own to
//! it, `x11rb`). Wayland lets a program minimize its window but not hide,
//! place or picture it: there it is minimized and the port says the rest.

use gpui::Window;

// Each system's own way, in a file of its own.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::*;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::*;
#[cfg(not(any(target_os = "macos", windows)))]
mod x11;
#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) use x11::*;

/// Elsewhere GPUI's own bounds say where the window is.
#[cfg(not(windows))]
pub fn frames(_: &Window) -> Option<(gpui::Bounds<gpui::Pixels>, gpui::Bounds<gpui::Pixels>)> {
    None
}

/// The folder `path` goes in, made where it is not there.
fn folder_for(path: &std::path::Path) -> Result<(), String> {
    match path.parent() {
        Some(folder) => std::fs::create_dir_all(folder)
            .map_err(|e| format!("{} could not be made: {e}", folder.display())),
        None => Ok(()),
    }
}

/// `bgrx` (blue, green, red and a byte the system leaves unset, as Windows
/// and X11 both hand a window's pixels back), `width` by `height`, saved as
/// an opaque PNG at `path`.
#[cfg(not(target_os = "macos"))]
fn save_png(path: &std::path::Path, width: u32, height: u32, bgrx: Vec<u8>) -> Result<(), String> {
    folder_for(path)?;
    let mut rgba = bgrx;
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&rgba))
        .map_err(|e| format!("the picture could not be saved: {e}"))
}
