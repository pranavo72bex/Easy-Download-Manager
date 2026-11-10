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

/// The window's `NSWindow`, through the system's handle.
#[cfg(target_os = "macos")]
fn ns_window(window: &Window) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    // The system's handle, not GPUI's own of the same name.
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: `ns_view` is the live content view of `window`, which this
    // borrow of the window keeps open; only its window is read, on the main
    // thread, where this runs.
    let view: &objc2_app_kit::NSView = unsafe { appkit.ns_view.cast().as_ref() };
    view.window()
}

/// Puts `window` away without closing it (`-[NSWindow orderOut:]`), as the
/// egui window hides into the tray.
#[cfg(target_os = "macos")]
pub fn hide(window: &Window) {
    if let Some(ns_window) = ns_window(window) {
        ns_window.orderOut(None);
    }
}

/// The window's `HWND`, through the system's handle.
#[cfg(windows)]
pub(crate) fn hwnd(window: &Window) -> Option<windows_sys::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return None;
    };
    Some(win32.hwnd.get() as windows_sys::Win32::Foundation::HWND)
}

/// Puts `window` away without closing it (`ShowWindow(SW_HIDE)`): off the
/// screen and the taskbar, as the egui window hides into the tray.
#[cfg(windows)]
pub fn hide(window: &Window) {
    if let Some(hwnd) = hwnd(window) {
        // SAFETY: `hwnd` is the live window this borrow keeps open; called on
        // the thread that made it.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(hwnd, 0) };
    }
}

/// The window's X11 id, where it is an X11 window.
#[cfg(not(any(target_os = "macos", windows)))]
fn x_window(window: &Window) -> Option<u32> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Xcb(xcb) => Some(xcb.window.get()),
        RawWindowHandle::Xlib(xlib) => u32::try_from(xlib.window).ok(),
        _ => None,
    }
}

/// `act` on the X server, through a connection of dm's own, made for it
/// and closed after.
#[cfg(not(any(target_os = "macos", windows)))]
fn on_x<T>(
    act: impl FnOnce(&x11rb::rust_connection::RustConnection) -> Result<T, String>,
) -> Result<T, String> {
    use x11rb::connection::Connection;
    let (connection, _) =
        x11rb::connect(None).map_err(|e| format!("the X server could not be reached: {e}"))?;
    let done = act(&connection)?;
    connection.flush().map_err(|e| e.to_string())?;
    Ok(done)
}

/// A request with no reply, waited on until the server has carried it out:
/// left in the connection's buffer as it closed, an unmap was lost.
#[cfg(not(any(target_os = "macos", windows)))]
fn done<C: x11rb::connection::RequestConnection>(
    sent: Result<x11rb::cookie::VoidCookie<'_, C>, x11rb::errors::ConnectionError>,
) -> Result<(), String> {
    sent.map_err(|e| e.to_string())?
        .check()
        .map_err(|e| e.to_string())
}

/// On X11 the window is unmapped: off the screen and the taskbar, as the
/// egui window hides into the tray. On Wayland it is minimized, which its
/// desktop's taskbar brings back.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn hide(window: &Window) {
    use x11rb::protocol::xproto::ConnectionExt;
    let unmapped = x_window(window).map(|id| on_x(|x| done(x.unmap_window(id))));
    if !matches!(unmapped, Some(Ok(()))) {
        window.minimize_window();
    }
}

/// Brings `window` back from the tray before it is put in front: hidden on
/// Windows, it is shown again (`ShowWindow(SW_SHOW)`); elsewhere putting it
/// in front is enough.
pub fn show(window: &Window) {
    #[cfg(windows)]
    if let Some(hwnd) = hwnd(window) {
        // SAFETY: as in `hide`.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(hwnd, 5) };
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    if let Some(id) = x_window(window) {
        use x11rb::protocol::xproto::ConnectionExt;
        let _ = on_x(|x| done(x.map_window(id)));
    }
    #[cfg(target_os = "macos")]
    let _ = window;
}

/// Puts `window`'s top-left corner at (`x`, `y`), in the points of the
/// screen whose top-left is (0, 0), as `ui.windows` reports where windows
/// stand.
#[cfg(target_os = "macos")]
pub fn move_to(window: &Window, x: f32, y: f32) -> Result<(), String> {
    let ns_window = ns_window(window).ok_or("the window has no system window")?;
    let main = objc2::MainThreadMarker::new().ok_or("not on the main thread")?;
    // AppKit counts from the bottom of the first screen, upwards.
    let screens = objc2_app_kit::NSScreen::screens(main);
    let first = screens.firstObject().ok_or("there is no screen")?;
    let height = first.frame().size.height;
    ns_window.setFrameTopLeftPoint(objc2_foundation::NSPoint::new(
        f64::from(x),
        height - f64::from(y),
    ));
    Ok(())
}

/// Windows counts in the screen's pixels: points times the window's scale.
#[cfg(windows)]
pub fn move_to(window: &Window, x: f32, y: f32) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    };
    let hwnd = hwnd(window).ok_or("the window has no system window")?;
    let scale = window.scale_factor();
    // SAFETY: as in `hide`.
    let done = unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            (x * scale).round() as i32,
            (y * scale).round() as i32,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
    if done == 0 {
        Err("the window could not be moved".into())
    } else {
        Ok(())
    }
}

/// X11 counts in the screen's pixels: points times the window's scale.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn move_to(window: &Window, x: f32, y: f32) -> Result<(), String> {
    use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt};
    let id = x_window(window).ok_or("Wayland does not let a program place its own window")?;
    let scale = window.scale_factor();
    let to = ConfigureWindowAux::new()
        .x((x * scale).round() as i32)
        .y((y * scale).round() as i32);
    on_x(|conn| done(conn.configure_window(id, &to)))
}

/// Pictures `window` as it stands on screen, its frame without its shadow,
/// into the PNG at `path` (the system's `screencapture`).
#[cfg(target_os = "macos")]
pub fn picture(window: &Window, path: &std::path::Path) -> Result<(), String> {
    let ns_window = ns_window(window).ok_or("the window has no system window")?;
    let number = ns_window.windowNumber();
    folder_for(path)?;
    let done = std::process::Command::new("/usr/sbin/screencapture")
        .arg("-x")
        .arg("-o")
        .arg(format!("-l{number}"))
        .arg(path)
        .status()
        .map_err(|e| format!("the window could not be pictured: {e}"))?;
    if done.success() && path.is_file() {
        Ok(())
    } else {
        Err("the window could not be pictured".into())
    }
}

/// The window drawn into a bitmap of its own (`PrintWindow`, with what the
/// compositor holds), saved as a PNG.
#[cfg(windows)]
pub fn picture(window: &Window, path: &std::path::Path) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let hwnd = hwnd(window).ok_or("the window has no system window")?;
    // SAFETY: every handle is made here, used on this thread and released
    // before returning; `hwnd` is the live window this borrow keeps open.
    let pixels = unsafe {
        let mut rect = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return Err("the window's size could not be read".into());
        }
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        if width <= 0 || height <= 0 {
            return Err("the window has no size".into());
        }
        let screen = GetDC(std::ptr::null_mut());
        let memory = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(memory, bitmap);
        // PW_RENDERFULLCONTENT: what DirectComposition draws, too.
        let printed = PrintWindow(hwnd, memory, 2 as PRINT_WINDOW_FLAGS);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        // Negative: top row first.
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        let mut bgra = vec![0u8; (width * height * 4) as usize];
        let read = GetDIBits(
            memory,
            bitmap,
            0,
            height as u32,
            bgra.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(memory, old);
        DeleteObject(bitmap);
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);
        if printed == 0 || read == 0 {
            return Err("the window could not be pictured".into());
        }
        (width as u32, height as u32, bgra)
    };
    let (width, height, bgra) = pixels;
    save_png(path, width, height, bgra)
}

/// The window's pixels as the X server holds them (`GetImage`), saved as
/// a PNG.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn picture(window: &Window, path: &std::path::Path) -> Result<(), String> {
    use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};
    let id = x_window(window).ok_or("Wayland gives a program no picture of its own window")?;
    let (width, height, bgrx) = on_x(|x| {
        let geometry = x
            .get_geometry(id)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let image = x
            .get_image(
                ImageFormat::Z_PIXMAP,
                id,
                0,
                0,
                geometry.width,
                geometry.height,
                !0,
            )
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| format!("the window could not be pictured: {e}"))?;
        Ok((
            u32::from(geometry.width),
            u32::from(geometry.height),
            image.data,
        ))
    })?;
    if bgrx.len() < (width * height * 4) as usize {
        return Err("the window's pixels are not 32 bits each".into());
    }
    save_png(path, width, height, bgrx)
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
