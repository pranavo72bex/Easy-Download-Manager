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
pub(crate) fn ns_window(window: &Window) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
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

/// Whether closing `window` has nowhere to put it: a Wayland window (no X11
/// id) on a compositor that ignores minimize (sway, Hyprland and the other
/// tiling ones, `dm_desktop_base::tray::minimize_is_ignored`), where a
/// window put away into the tray would stay on the screen looking dead.
/// Never elsewhere.
pub fn nowhere_to_hide(window: &Window) -> bool {
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        x_window(window).is_none()
            && dm_desktop_base::tray::minimize_is_ignored(
                &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            )
    }
    #[cfg(any(target_os = "macos", windows))]
    {
        let _ = window;
        false
    }
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

/// `hwnd`'s whole rectangle on the screen (`GetWindowRect`, with the borders
/// Windows keeps invisible around it for resizing, 7 pixels at 100%) and
/// the part of it seen (the compositor's extended frame bounds; the whole
/// where the compositor does not say), in the screen's pixels.
#[cfg(windows)]
fn rects(
    hwnd: windows_sys::Win32::Foundation::HWND,
) -> Option<(
    windows_sys::Win32::Foundation::RECT,
    windows_sys::Win32::Foundation::RECT,
)> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    // SAFETY: `hwnd` is a live window of this thread's (each caller holds
    // its `Window`); each call writes only the value handed to it.
    unsafe {
        let mut whole: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut whole) == 0 {
            return None;
        }
        let mut seen: RECT = std::mem::zeroed();
        let framed = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut seen as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        ) == 0
            && seen.right > seen.left
            && seen.bottom > seen.top;
        Some((whole, if framed { seen } else { whole }))
    }
}

/// Where Windows has `window` on the screen, in points: its frame as it is
/// seen (the rectangle `move_to` places; the invisible resizing borders
/// left out, so a dialog pictured from the screen holds nothing of what lies
/// behind it) and its inside (the client area). GPUI's own bounds are the
/// inside only, so a dialog moved to (x, y) was reported 8 and 31 points
/// further on, past its border and title bar.
#[cfg(windows)]
pub fn frames(window: &Window) -> Option<(gpui::Bounds<gpui::Pixels>, gpui::Bounds<gpui::Pixels>)> {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;
    let hwnd = hwnd(window)?;
    let (_, seen) = rects(hwnd)?;
    // SAFETY: as in `hide`; each call writes only the value handed to it.
    let (client, origin) = unsafe {
        let mut client: RECT = std::mem::zeroed();
        let mut origin = POINT { x: 0, y: 0 };
        if GetClientRect(hwnd, &mut client) == 0 || ClientToScreen(hwnd, &mut origin) == 0 {
            return None;
        }
        (client, origin)
    };
    let scale = window.scale_factor();
    let bounds = |left: i32, top: i32, right: i32, bottom: i32| {
        gpui::Bounds::from_corners(
            gpui::point(gpui::px(left as f32 / scale), gpui::px(top as f32 / scale)),
            gpui::point(
                gpui::px(right as f32 / scale),
                gpui::px(bottom as f32 / scale),
            ),
        )
    };
    Some((
        bounds(seen.left, seen.top, seen.right, seen.bottom),
        bounds(
            origin.x,
            origin.y,
            origin.x + client.right,
            origin.y + client.bottom,
        ),
    ))
}

/// Elsewhere GPUI's own bounds say where the window is.
#[cfg(not(windows))]
pub fn frames(_: &Window) -> Option<(gpui::Bounds<gpui::Pixels>, gpui::Bounds<gpui::Pixels>)> {
    None
}

/// Windows counts in the screen's pixels: points times the window's scale.
/// The frame as it is seen goes to (x, y), as `frames` reports it.
#[cfg(windows)]
pub fn move_to(window: &Window, x: f32, y: f32) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    };
    let hwnd = hwnd(window).ok_or("the window has no system window")?;
    let scale = window.scale_factor();
    // The invisible border at the left and top, which the whole rectangle
    // placed by `SetWindowPos` starts with.
    let (left, top) = rects(hwnd).map_or((0, 0), |(whole, seen)| {
        (seen.left - whole.left, seen.top - whole.top)
    });
    // SAFETY: as in `hide`.
    let done = unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            (x * scale).round() as i32 - left,
            (y * scale).round() as i32 - top,
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
    place(window, x, y)
}

/// Puts `window`'s own top-left corner (inside the frame the window manager
/// draws) at (`x`, `y`), as `ui.windows` reports where windows stand, and
/// says so in its `WM_NORMAL_HINTS`: a position the program chose, with
/// static gravity. GPUI shows a window before dm can place it and gives no
/// position, so a window manager put a dialog where it found room (openbox:
/// the screen's bottom-left corner) rather than over dm's window. Asked
/// before the window manager has taken the window, the hint places it; after,
/// the move does. The hints GPUI set (the smallest and largest sizes) stay.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn place(window: &Window, x: f32, y: f32) -> Result<(), String> {
    use x11rb::properties::{WmSizeHints, WmSizeHintsSpecification};
    use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt, Gravity};
    let id = x_window(window).ok_or("Wayland does not let a program place its own window")?;
    let scale = window.scale_factor();
    let (x, y) = ((x * scale).round() as i32, (y * scale).round() as i32);
    on_x(|conn| {
        let mut hints = WmSizeHints::get_normal_hints(conn, id)
            .map_err(|e| e.to_string())?
            .reply()
            .ok()
            .flatten()
            .unwrap_or_default();
        hints.position = Some((WmSizeHintsSpecification::ProgramSpecified, x, y));
        hints.win_gravity = Some(Gravity::STATIC);
        done(hints.set_normal_hints(conn, id))?;
        done(conn.configure_window(id, &ConfigureWindowAux::new().x(x).y(y)))
    })
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
/// compositor holds), cut to what is seen of it (the system's invisible
/// borders for resizing left out, as a person sees the window), saved as a
/// PNG.
#[cfg(windows)]
pub fn picture(window: &Window, path: &std::path::Path) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    let hwnd = hwnd(window).ok_or("the window has no system window")?;
    let (rect, frame) = rects(hwnd).ok_or("the window's size could not be read")?;
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    if width <= 0 || height <= 0 {
        return Err("the window has no size".into());
    }
    // What is seen of it, in the bitmap's pixels.
    let seen = (
        (frame.left - rect.left).clamp(0, width),
        (frame.top - rect.top).clamp(0, height),
        (frame.right - rect.left).clamp(0, width),
        (frame.bottom - rect.top).clamp(0, height),
    );
    // SAFETY: every handle is made here, used on this thread and released
    // before returning; `hwnd` is the live window this borrow keeps open.
    let bgra = unsafe {
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
        bgra
    };
    let (left, top, right, bottom) = seen;
    if right <= left || bottom <= top {
        return save_png(path, width as u32, height as u32, bgra);
    }
    let row = width as usize * 4;
    let cut: Vec<u8> = (top..bottom)
        .flat_map(|y| {
            let start = y as usize * row + left as usize * 4;
            bgra[start..start + (right - left) as usize * 4]
                .iter()
                .copied()
        })
        .collect();
    save_png(path, (right - left) as u32, (bottom - top) as u32, cut)
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
