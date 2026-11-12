// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

// The notification's Open and Open folder (Linux).
use dm_desktop_base::desktop::WindowServices;
#[cfg(all(unix, not(target_os = "macos")))]
use dm_shell::shell;

/// What the window's own crates do for the desktop's services
/// (`dm_desktop_base::desktop::Desktop`): the clipboard (arboard), the file
/// choosers (rfd) and, away from Windows, notifications (notify-rust).
pub(super) struct EguiWindow;

impl WindowServices for EguiWindow {
    fn clipboard(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    fn choose_app(&self) -> Option<std::path::PathBuf> {
        let dialog = rfd::FileDialog::new().set_title("Open with");
        #[cfg(target_os = "macos")]
        let dialog = dialog
            .set_directory("/Applications")
            .add_filter("Applications", &["app"]);
        #[cfg(target_os = "windows")]
        let dialog = dialog.add_filter("Apps", &["exe"]);
        dialog.pick_file()
    }

    fn choose_folder(&self) -> Option<std::path::PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }

    fn choose_file(&self, save: bool, suggested: &str) -> Option<std::path::PathBuf> {
        // What is asked for decides the filter: a settings file is JSON,
        // and was hidden behind the download lists' filter.
        let dialog = if suggested.ends_with(".json") {
            rfd::FileDialog::new().add_filter("dm settings", &["json"])
        } else {
            rfd::FileDialog::new().add_filter("Download lists", &["dmlist", "txt"])
        };
        if save {
            dialog.set_file_name(suggested).save_file()
        } else {
            dialog.pick_file()
        }
    }

    /// Not called on Windows, where dm posts its own toasts.
    fn say(&self, note: dm_desktop::quiet::Note) {
        #[cfg(not(windows))]
        say(note);
        #[cfg(windows)]
        let _ = note;
    }
}

/// Starts the system's drag of finished files out of the window, into
/// Finder or Explorer, a mail or a chat. Called while the pointer is down,
/// from the frame that saw the drag begin.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn drag_out(
    frame: &eframe::Frame,
    files: Vec<std::path::PathBuf>,
) -> Result<(), String> {
    drag::start_drag(
        frame,
        drag::DragItem::Files(files),
        drag::Image::Raw(DRAG_ICON.to_vec()),
        |_, _| {},
        drag::Options::default(),
    )
    .map_err(|e| format!("the file could not be dragged out: {e}"))
}

/// On Linux the drag library needs a GTK window, and dm's is not one: on
/// X11 dm speaks the drag protocol itself (`xdnd`); on Wayland the drag has
/// to begin on winit's own connection, with the press's serial, so dm's
/// patched winit starts it (`dm_request_drag_out`).
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) fn drag_out(
    frame: &eframe::Frame,
    files: Vec<std::path::PathBuf>,
) -> Result<(), String> {
    use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
    #[cfg(target_os = "linux")]
    if let Ok(handle) = frame.display_handle() {
        match handle.as_raw() {
            RawDisplayHandle::Xlib(xlib) => {
                let display = xlib.display.map_or(std::ptr::null_mut(), |d| d.as_ptr());
                return dm_desktop::xdnd::start(display, files);
            }
            RawDisplayHandle::Wayland(_) => {
                winit::platform::wayland::dm_request_drag_out(dm_desktop::xdnd::uri_list(&files));
                return Ok(());
            }
            _ => {}
        }
    }
    let _ = (frame, files);
    Err(
        dm_desktop::i18n::t("Dragging out isn't available here. Use right-click › Open folder.")
            .into(),
    )
}

/// What a dragged file looks like under the pointer: dm's icon, small.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) const DRAG_ICON: &[u8] = include_bytes!("../icons/32x32.png");

/// Says a toast: one whose click opens a finished file, or a plain one.
#[cfg(not(windows))]
fn say(note: dm_desktop::quiet::Note) {
    let dm_desktop::quiet::Note {
        title, body, path, ..
    } = note;
    let Some(path) = path else {
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new()
                .appname("dm")
                .summary(&title)
                .body(&body)
                .show();
        });
        return;
    };
    /// Notifications waited on at once: each holds a thread until it is
    /// clicked or closed, and some servers keep them a long time.
    static WAITING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    const MOST_WAITING: usize = 8;
    std::thread::spawn(move || {
        let mut notification = notify_rust::Notification::new();
        notification.appname("dm").summary(&title).body(&body);
        #[cfg(all(unix, not(target_os = "macos")))]
        if !path.is_empty() {
            // The body's click, as the servers name it, and a button for
            // the folder: Open File and Open Folder on the toast itself.
            notification.action("default", "Open");
            notification.action("folder", "Open folder");
        }
        let Ok(handle) = notification.show() else {
            return;
        };
        #[cfg(target_os = "macos")]
        let _ = (handle, &path, &WAITING, MOST_WAITING);
        #[cfg(not(target_os = "macos"))]
        {
            use std::sync::atomic::Ordering;
            if path.is_empty() || WAITING.fetch_add(1, Ordering::AcqRel) >= MOST_WAITING {
                WAITING.fetch_sub(1, Ordering::AcqRel);
                return;
            }
            let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
                if response.is_default_action() {
                    let _ = shell::open_file(&path);
                } else if matches!(response, notify_rust::NotificationResponse::Action(key) if key == "folder")
                {
                    let _ = shell::reveal_in_folder(&path);
                }
            });
            WAITING.fetch_sub(1, Ordering::AcqRel);
        }
    });
}
