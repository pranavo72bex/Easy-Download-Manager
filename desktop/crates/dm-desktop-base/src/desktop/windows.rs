// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Windows: dm's own toasts.

use super::*;

/// Says a toast: one whose click opens a finished file, or a plain one.
/// Posted by dm itself on Windows (`crate::toast`): notify-rust's kept
/// three handles for every toast (gaps row 156).
pub(super) fn say_on_windows(note: Note) {
    let Note {
        title,
        body,
        path,
        retry,
    } = note;
    // In the window's language, which is this thread's.
    let button = crate::i18n::t("Open folder");
    let retry_words = crate::i18n::t("Retry");
    // On a thread of its own, as before: the session's first toast starts
    // Windows' runtime, which takes a moment the window should not wait.
    std::thread::spawn(move || {
        match path.filter(|p| !p.is_empty()) {
            // A click opens the file, its Open folder shows it in Explorer
            // (gaps row U15).
            Some(path) => {
                let _ = crate::toast::show_file(&title, &body, button, move |what| {
                    let _ = if what == crate::toast::FOLDER {
                        shell::reveal_in_folder(&path)
                    } else {
                        shell::open_file(&path)
                    };
                });
            }
            None => match retry {
                // Its Retry tries the row again, through the running dm's
                // inbox; a click on the toast itself brings dm forward.
                Some(id) => {
                    let _ = crate::toast::show_file(&title, &body, retry_words, move |what| {
                        if what == crate::toast::FOLDER {
                            shell::ask_running_to_retry(&dm_shell::settings::home(), id);
                        }
                    });
                }
                None => {
                    let _ = crate::toast::show(&title, &body, None::<fn()>);
                }
            },
        }
    });
}
