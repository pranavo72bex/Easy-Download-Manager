// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Windows: through Win32, on the window's `HWND`.

use super::*;

/// The window's `HWND`, through the system's handle.
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
pub fn hide(window: &Window) {
    if let Some(hwnd) = hwnd(window) {
        // SAFETY: `hwnd` is the live window this borrow keeps open; called on
        // the thread that made it.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(hwnd, 0) };
    }
}

/// `hwnd`'s whole rectangle on the screen (`GetWindowRect`, with the borders
/// Windows keeps invisible around it for resizing, 7 pixels at 100%) and
/// the part of it seen (the compositor's extended frame bounds; the whole
/// where the compositor does not say), in the screen's pixels.
pub(super) fn rects(
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

/// Windows counts in the screen's pixels: points times the window's scale.
/// The frame as it is seen goes to (x, y), as `frames` reports it.
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

/// The window drawn into a bitmap of its own (`PrintWindow`, with what the
/// compositor holds), cut to what is seen of it (the system's invisible
/// borders for resizing left out, as a person sees the window), saved as a
/// PNG.
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

/// A hidden window can always be put away into the tray here.
pub fn nowhere_to_hide(_: &Window) -> bool {
    false
}

/// Brings `window` back from the tray before it is put in front: hidden, it
/// is shown again (`ShowWindow(SW_SHOW)`).
pub fn show(window: &Window) {
    if let Some(hwnd) = hwnd(window) {
        // SAFETY: as in `hide`.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(hwnd, 5) };
    }
}
