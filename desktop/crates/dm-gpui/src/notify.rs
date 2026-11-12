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

/// Says a notification: one whose click opens a finished file, with an
/// Open folder button, or a plain one. Off the window's thread: the system
/// may take a moment.
///
/// The click is heard by waiting on the notification in step, on its own
/// thread: Linux's notification servers answer so, and macOS's centre does
/// too while the program's main run loop runs, which GPUI's does (gaps row
/// U15: before, a click on macOS only brought dm forward).
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
    // In the window's language, which is this thread's.
    let buttons = buttons();
    std::thread::spawn(move || {
        use std::sync::atomic::Ordering;
        let mut notification = notify_rust::Notification::new();
        notification.appname("dm").summary(&title).body(&body);
        let waited = !path.is_empty() && WAITING.fetch_add(1, Ordering::AcqRel) < MOST_WAITING;
        if !path.is_empty() && !waited {
            WAITING.fetch_sub(1, Ordering::AcqRel);
        }
        if waited {
            for (key, words) in buttons {
                notification.action(key, words);
            }
        }
        let Ok(handle) = notification.show() else {
            if waited {
                WAITING.fetch_sub(1, Ordering::AcqRel);
            }
            return;
        };
        if !waited {
            // Dropped, it is posted as it is (macOS) or left to the server.
            drop(handle);
            return;
        }
        let _ =
            handle.wait_for_response(
                |response: &notify_rust::NotificationResponse| match response {
                    notify_rust::NotificationResponse::Default => {
                        let _ = dm_shell::shell::open_file(&path);
                    }
                    notify_rust::NotificationResponse::Action(key) if key == "folder" => {
                        let _ = dm_shell::shell::reveal_in_folder(&path);
                    }
                    notify_rust::NotificationResponse::Action(key) if key == "default" => {
                        let _ = dm_shell::shell::open_file(&path);
                    }
                    _ => {}
                },
            );
        WAITING.fetch_sub(1, Ordering::AcqRel);
    });
}

/// The buttons on a finished file's notification, by the keys its answer
/// names them with, in this thread's language (the window's). Linux's servers take the body's
/// click as the `default` action, which some draw as a button of its own;
/// macOS draws one button, and a click on the body is its own answer.
fn buttons() -> Vec<(&'static str, &'static str)> {
    use dm_desktop_base::i18n::t;
    if cfg!(target_os = "macos") {
        vec![("folder", t("Open folder"))]
    } else {
        vec![("default", t("Open")), ("folder", t("Open folder"))]
    }
}
