// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Linux: on X11 through the X server itself (a connection of dm's own to it, `x11rb`);
//! on Wayland the window is minimized and the rest is refused.

use super::*;

/// The window's X11 id, where it is an X11 window.
pub(super) fn x_window(window: &Window) -> Option<u32> {
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
pub(super) fn on_x<T>(
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
pub(super) fn done<C: x11rb::connection::RequestConnection>(
    sent: Result<x11rb::cookie::VoidCookie<'_, C>, x11rb::errors::ConnectionError>,
) -> Result<(), String> {
    sent.map_err(|e| e.to_string())?
        .check()
        .map_err(|e| e.to_string())
}

/// On X11 the window is unmapped: off the screen and the taskbar, as the
/// egui window hides into the tray. On Wayland it is minimized, which its
/// desktop's taskbar brings back.
pub fn hide(window: &Window) {
    use x11rb::protocol::xproto::ConnectionExt;
    let unmapped = x_window(window).map(|id| on_x(|x| done(x.unmap_window(id))));
    if !matches!(unmapped, Some(Ok(()))) {
        window.minimize_window();
    }
}

/// X11 counts in the screen's pixels: points times the window's scale.
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

/// The window's pixels as the X server holds them (`GetImage`), saved as
/// a PNG.
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

/// Whether closing `window` has nowhere to put it: a Wayland window (no X11
/// id) on a compositor that ignores minimize (sway, Hyprland and the other
/// tiling ones, `dm_desktop_base::tray::minimize_is_ignored`), where a
/// window put away into the tray would stay on the screen looking dead.
pub fn nowhere_to_hide(window: &Window) -> bool {
    x_window(window).is_none()
        && dm_desktop_base::tray::minimize_is_ignored(
            &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        )
}

/// Brings `window` back from the tray before it is put in front: mapped
/// again on X11.
pub fn show(window: &Window) {
    if let Some(id) = x_window(window) {
        use x11rb::protocol::xproto::ConnectionExt;
        let _ = on_x(|x| done(x.map_window(id)));
    }
}
