// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Android (gaps row N17): the window's notifier and clipboard, through what the app handed
//! in (`dm_desktop_base::phone`), in `notify.rs`'s and `services.rs`'s place. A notification
//! is the phone's own, its click and Retry heard by the app. The window's choosers
//! (`services::ask`) on a phone (`ask`): GPUI's file prompt for a file to read, the save folder
//! for a file to write, the folders dm can download into for a folder.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use dm_desktop_base::i18n::{t, tf};
use dm_desktop_base::quiet::Note;
use gpui::{AnyWindowHandle, App, AsyncApp, PathPromptOptions, PromptButton, PromptLevel};

use crate::platform::services::Asked;

/// Nothing to claim: the app's notifications are posted under its own name.
pub fn claim() {}

/// Says `note` in the phone's notification shade.
pub fn say(note: Note) {
    dm_desktop_base::phone::notify(note);
}

/// The phone's clipboard text.
pub fn clipboard() -> Option<String> {
    dm_desktop_base::phone::clipboard()
}

/// Whether the phone's clipboard can be read now: only while dm's window is in front.
pub fn clipboard_readable() -> bool {
    dm_desktop_base::phone::clipboard_readable()
}

/// Puts `text` on the phone's clipboard.
pub fn set_clipboard(text: &str) {
    dm_desktop_base::phone::set_clipboard(text);
}

/* ------------------------------------------------------------- choosers */

/// What the phone's folder choice says under its question (gaps row N9): why only these.
const ONLY_THESE: &str = "On a phone dm downloads only into these folders.";

/// A file dm writes for the person (Export, Back up dm, Save settings to a file), asked of
/// `services::ask` with no window to say a refusal in: the place it is written, the save
/// folder dm has on the phone (`dm_desktop_base::phone::save_place`); the window's own
/// message then says where it went. None, and the reason noted, where the phone has none.
pub fn save_place(asked: &Asked) -> Option<PathBuf> {
    match dm_desktop_base::phone::save_place(asked.name.as_deref().unwrap_or_default()) {
        Ok(path) => Some(path),
        Err(why) => {
            dm_core::note!(warn, "{why}");
            None
        }
    }
}

/// The phone's answer to `asked` (`services::ask_then`, `ask_then_in`); what is said is said in
/// `window`, else the window in front, once the click that asked is done:
/// - a file to read: GPUI's own file prompt (`prompt_for_paths`), which gpui-android answers
///   with the system's document picker, the document copied into the app's cache first, so
///   the answer is a path dm reads as any other;
/// - a file to write: the save folder (`save_place`), as GPUI's Android platform has no
///   `prompt_for_new_path`;
/// - a folder: one of the folders dm can download into on the phone, offered by name in GPUI's
///   prompt, as GPUI's picker gives no folders and a folder of the storage picker's is no path.
///
/// A refusal is said in the window; cancelled is none.
pub fn ask(
    asked: Asked,
    window: Option<AnyWindowHandle>,
    cx: &mut App,
) -> Pin<Box<dyn Future<Output = Option<PathBuf>>>> {
    let window = window
        .or_else(|| cx.active_window())
        .or_else(|| cx.windows().into_iter().next());
    let paths = (!asked.save && !asked.folder).then(|| {
        cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        })
    });
    let task = cx.spawn(async move |cx: &mut AsyncApp| {
        if asked.save {
            return match dm_desktop_base::phone::save_place(
                asked.name.as_deref().unwrap_or_default(),
            ) {
                Ok(path) => Some(path),
                Err(why) => {
                    cx.update(|cx| tell(window, &why, cx));
                    None
                }
            };
        }
        if let Some(paths) = paths {
            return match paths.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(why)) => {
                    let why = tf("The file couldn’t be chosen: {}", &[&why]);
                    cx.update(|cx| tell(window, &why, cx));
                    None
                }
            };
        }
        let folders = match dm_desktop_base::phone::download_folders() {
            Ok(folders) => folders,
            Err(why) => {
                cx.update(|cx| tell(window, &why, cx));
                return None;
            }
        };
        let mut answers: Vec<PromptButton> = folders
            .iter()
            .map(|folder| PromptButton::new(folder.display().to_string()))
            .collect();
        answers.push(PromptButton::cancel(t("Cancel")));
        let answer = cx.update(|cx| {
            window?
                .update(cx, |_, window, cx| {
                    window.prompt(
                        PromptLevel::Info,
                        t("Choose a folder"),
                        Some(t(ONLY_THESE)),
                        &answers,
                        cx,
                    )
                })
                .ok()
        })?;
        let chosen = answer.await.ok()?;
        folders.get(chosen).cloned()
    });
    Box::pin(task)
}

/// `why` said in `window`, with OK.
fn tell(window: Option<AnyWindowHandle>, why: &str, cx: &mut App) {
    dm_core::note!(warn, "{why}");
    if let Some(window) = window {
        let _ = window.update(cx, |_, window, cx| {
            let answer = window.prompt(
                PromptLevel::Warning,
                why,
                None,
                &[PromptButton::ok(t("OK"))],
                cx,
            );
            // Held until OK closes it; nothing follows.
            cx.spawn(async move |_| {
                let _ = answer.await;
            })
            .detach();
        });
    }
}
