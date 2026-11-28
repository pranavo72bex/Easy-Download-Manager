// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Windows ends a session (a logoff, a shutdown or restart, an installer's
//! Restart Manager) by asking each top-level window, not with a signal, and
//! may end the process as soon as its windows have answered. A console's
//! logoff and shutdown events never reach a process that has the window
//! system loaded, as EDM has, and `--headless` has no window of GPUI's to
//! hear it: EDM keeps a hidden window of its own, on a thread of its own,
//! that does what Exit does to the downloads before it answers (gaps row
//! 186). It is never shown, and it never holds the session up.

use std::cell::RefCell;
use std::sync::{mpsc, Arc, Once};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, MSG,
    WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

/// What is done as the session ends, before the window answers.
pub(super) type Ending = Arc<dyn Fn() + Send + Sync>;

thread_local! {
    /// The watching thread's own ending: its window's procedure runs on it.
    static ENDING: RefCell<Option<Ending>> = const { RefCell::new(None) };
}

/// The hidden window's class, which `scripts/stress-check.py quit` also
/// finds it by.
const CLASS: &str = "edm-session-end";

/// Starts the hidden window and its thread; `end` runs when the session
/// ends. The window's handle, or `None` where it could not be made.
pub(super) fn watch(end: Ending) -> Option<isize> {
    let (made, answer) = mpsc::channel();
    std::thread::Builder::new()
        .name("edm-session-end".into())
        .spawn(move || {
            ENDING.with(|ending| *ending.borrow_mut() = Some(end));
            let window = make_window();
            let _ = made.send(window.map(|w| w as isize));
            if window.is_none() {
                return;
            }
            // SAFETY: an all-zero MSG is a valid value for GetMessageW to fill.
            let mut message: MSG = unsafe { std::mem::zeroed() };
            // SAFETY: `message` is a live MSG; a null window takes every
            // message of this thread. -1 (an error) and 0 (WM_QUIT) end it.
            while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
                // SAFETY: a message GetMessageW has just filled.
                unsafe { DispatchMessageW(&message) };
            }
        })
        .ok()?;
    answer.recv().ok().flatten()
}

/// The hidden window, made on the calling thread, whose messages it gets.
fn make_window() -> Option<HWND> {
    static REGISTERED: Once = Once::new();
    let class: Vec<u16> = CLASS.encode_utf16().chain(Some(0)).collect();
    // SAFETY: a null name asks for this executable's own module.
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    REGISTERED.call_once(|| {
        let kind = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(heard),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        // SAFETY: `kind` and the class name it points at are alive for the
        // call, which copies them.
        unsafe { RegisterClassW(&kind) };
    });
    // A top-level window, as only those are asked as a session ends; never
    // shown (no WS_VISIBLE), and a tool window, so no taskbar button.
    // SAFETY: the class was registered above; its name is NUL-terminated and
    // alive for the call; no parent, menu or creation data.
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            std::ptr::null(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    (!window.is_null()).then_some(window)
}

/// The hidden window's procedure: the session may end at once (EDM never
/// holds it up), and once it is ending EDM's own ending is done before the
/// answer, since Windows may end the process as soon as it has one. A
/// session that was not ended after all (`WM_ENDSESSION` with false: another
/// program held it) changes nothing.
unsafe extern "system" fn heard(
    window: HWND,
    message: u32,
    ending: WPARAM,
    how: LPARAM,
) -> LRESULT {
    match message {
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            if ending != 0 {
                let end = ENDING.with(|end| end.borrow().clone());
                if let Some(end) = end {
                    // Nothing unwinds out into the window system.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(&*end));
                }
            }
            0
        }
        // SAFETY: the arguments as the window system passed them.
        _ => unsafe { DefWindowProcW(window, message, ending, how) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsWindowVisible, SendMessageW, ENDSESSION_LOGOFF,
    };

    #[test]
    fn the_session_ending_waits_for_edm_to_end_and_a_session_kept_ends_nothing() {
        let ended = Arc::new(AtomicBool::new(false));
        let window = watch({
            let ended = Arc::clone(&ended);
            Arc::new(move || {
                std::thread::sleep(Duration::from_millis(300));
                ended.store(true, Ordering::SeqCst);
            })
        })
        .expect("the hidden window") as HWND;
        let logoff = ENDSESSION_LOGOFF as LPARAM;
        // SAFETY: a window of this process, alive while its thread runs.
        unsafe {
            assert_eq!(IsWindowVisible(window), 0, "a window a person sees");
            assert_eq!(
                SendMessageW(window, WM_QUERYENDSESSION, 0, logoff),
                1,
                "EDM held the session up"
            );
            // Another program held the session: nothing is ended.
            SendMessageW(window, WM_ENDSESSION, 0, logoff);
            assert!(!ended.load(Ordering::SeqCst), "ended for a session kept");
            let asked = Instant::now();
            SendMessageW(window, WM_ENDSESSION, 1, logoff);
            // The answer came only once EDM had ended: Windows may end the
            // process as soon as it has one.
            assert!(ended.load(Ordering::SeqCst), "answered before EDM ended");
            assert!(asked.elapsed() >= Duration::from_millis(300));
        }
    }
}
