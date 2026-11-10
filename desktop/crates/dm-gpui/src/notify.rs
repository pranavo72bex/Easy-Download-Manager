// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The GPUI window's notifier (`dm_desktop_base::desktop::WindowServices`
//! takes a window's own, `docs/decisions.md` 2026-11-10): a notification
//! posted to the system's own notification centre (macOS's, a Linux
//! desktop's), "Download finished", "Download failed". On Windows dm posts
//! its own toasts (`dm_desktop_base::toast`), so this is not used there.

use dm_desktop_base::quiet::Note;

/// Claims dm's own name in the notification centre, once at start: on macOS
/// notifications are posted as the app with this bundle id.
pub fn claim() {
    #[cfg(target_os = "macos")]
    let _ = notify_rust::set_application("com.dm.desktop");
}

/// Says a notification: one whose click opens a finished file (on Linux,
/// with an Open folder button too), or a plain one. Off the window's
/// thread: the system may take a moment.
pub fn say(note: Note) {
    let Note { title, body, path } = note;
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
                    let _ = dm_shell::shell::open_file(&path);
                } else if matches!(response, notify_rust::NotificationResponse::Action(key) if key == "folder")
                {
                    let _ = dm_shell::shell::reveal_in_folder(&path);
                }
            });
            WAITING.fetch_sub(1, Ordering::AcqRel);
        }
    });
}
