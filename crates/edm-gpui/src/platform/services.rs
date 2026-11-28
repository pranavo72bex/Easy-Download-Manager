// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the GPUI window supplies to the desktop's services
//! (`edm_desktop_base::desktop::WindowServices`): the clipboard and
//! notifications (`notify.rs`); and the one way its file and folder choosers
//! are asked (`ask`): the system's own open and save panels, not waited for
//! (on macOS the NSOpenPanel and NSSavePanel, elsewhere `rfd`'s).
//!
//! One clipboard: EDM's own reads and copies (the watch, Add URL, Add links
//! from clipboard, Copy link, Copy details) all go through `arboard` here,
//! reached as `backend.platform`, so a test's stand-in answers for them.
//! GPUI's clipboard needs the window's thread, and the watch reads off it
//! every second (gaps row 183), where GPUI's Windows clipboard would copy a
//! picture on it too. On Linux under Wayland EDM's copies go through GPUI's
//! clipboard instead (`copy`, gaps row 218): `arboard` is built without
//! Wayland, and with no XWayland they went nowhere; copies are made on the
//! window's thread. GPUI's prompts are not used: they take no kind of file
//! and no starting folder. The text fields' own Copy and Paste are
//! gpui-component's, on GPUI's clipboard.
//!
//! On a phone (gaps row N17) the clipboard and notifications are the app's
//! (`phone.rs`), and so are the choosers (gaps row N9, `phone::ask`): GPUI's
//! file prompt for a file to read, which gpui-android answers with the
//! system's document picker; the phone's save folder for a file to write;
//! the folders EDM can download into, offered in GPUI's prompt, for a folder.

use std::path::PathBuf;
#[cfg(not(target_os = "android"))]
use std::sync::Mutex;

use edm_desktop_base::desktop::WindowServices;
use edm_desktop_base::i18n::t;
use edm_desktop_base::quiet::Note;

/// The GPUI window's services.
pub struct GpuiServices;

/// The clipboard EDM copies with, kept: on Linux what was copied is served by
/// the `arboard::Clipboard` that put it there, and would go with it.
#[cfg(not(target_os = "android"))]
static COPIER: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

impl WindowServices for GpuiServices {
    #[cfg(not(target_os = "android"))]
    fn clipboard(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    #[cfg(not(target_os = "android"))]
    fn set_clipboard(&self, text: &str) {
        let mut copier = edm_core::lock(&COPIER);
        if copier.is_none() {
            *copier = arboard::Clipboard::new().ok();
        }
        if let Some(clipboard) = copier.as_mut() {
            let _ = clipboard.set_text(text);
        }
    }

    /// The phone's, through the app.
    #[cfg(target_os = "android")]
    fn clipboard(&self) -> Option<String> {
        crate::platform::phone::clipboard()
    }

    /// The phone's, through the app.
    #[cfg(target_os = "android")]
    fn set_clipboard(&self, text: &str) {
        crate::platform::phone::set_clipboard(text);
    }

    /// On Linux under Wayland: GPUI speaks the compositor's clipboard,
    /// `arboard` here does not.
    fn copies_through_window(&self) -> bool {
        cfg!(target_os = "linux") && on_wayland(std::env::var_os("WAYLAND_DISPLAY").as_deref())
    }

    /// The system's own notification centre (`crate::platform::notify`).
    fn say(&self, note: Note) {
        #[cfg(not(windows))]
        crate::platform::notify::say(note);
        #[cfg(windows)]
        let _ = note;
    }
}

/// Whether `display` (`WAYLAND_DISPLAY`) names a Wayland display, as GPUI
/// takes it when it picks Wayland over X11.
fn on_wayland(display: Option<&std::ffi::OsStr>) -> bool {
    display.is_some_and(|d| !d.is_empty())
}

/// Puts `text` on the clipboard for one of EDM's own copies (Copy link, Copy
/// details): through `platform`, so a test's stand-in takes it, or through
/// GPUI's clipboard where `platform` says copies go through the window.
pub fn copy(platform: &dyn edm_shell::app::platform::Platform, text: &str, cx: &gpui::App) {
    if platform.copies_through_window() {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
    } else {
        platform.set_clipboard(text);
    }
}

/* ------------------------------------------------- panels, not waited for */

/// What a system panel is asked for: a folder, a file to open or a name to
/// save under, of a kind (`filter`: its name and endings), where it starts
/// and the name it offers.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(target_os = "android", allow(dead_code))]
pub struct Asked {
    /// A name to save under, rather than a file to open.
    pub save: bool,
    /// A folder, rather than a file.
    pub folder: bool,
    /// The kind of file: its name and the endings it takes.
    pub filter: Option<(&'static str, &'static [&'static str])>,
    /// The folder it opens in; the system's choice where none.
    pub directory: Option<PathBuf>,
    /// The name a save panel offers.
    pub name: Option<String>,
}

impl Asked {
    /// A folder, as Browse… asks for one.
    pub fn folder() -> Asked {
        Asked {
            folder: true,
            ..Asked::default()
        }
    }

    /// A list or a settings file to open, or a name to save one under,
    /// `suggested` offered: what is asked for decides the filter, a
    /// settings file being JSON.
    pub fn file(save: bool, suggested: &str) -> Asked {
        Asked {
            save,
            filter: Some(if suggested.ends_with(".json") {
                (t("EDM settings"), &["json"])
            } else {
                (t("Download lists"), &["dmlist", "txt"])
            }),
            name: (save && !suggested.is_empty()).then(|| suggested.to_string()),
            ..Asked::default()
        }
    }

    /// An app to open a file with (Open with…).
    pub fn app() -> Asked {
        Asked {
            #[cfg(target_os = "macos")]
            directory: Some(PathBuf::from("/Applications")),
            #[cfg(target_os = "macos")]
            filter: Some((t("Applications"), &["app"])),
            #[cfg(target_os = "windows")]
            filter: Some((t("Apps"), &["exe"])),
            ..Asked::default()
        }
    }
}

impl Asked {
    /// A backup of EDM to open, or a name to save one under (File › Back up EDM… and Restore…,
    /// gaps row W16).
    pub fn backup(save: bool) -> Asked {
        Asked {
            save,
            filter: Some((t("EDM backups"), &["dmbackup"])),
            name: save.then(|| edm_shell::backup::SUGGESTED.to_string()),
            ..Asked::default()
        }
    }
}

/// The path a script chose ahead (`ui.choose`) and when.
#[cfg(not(test))]
static CHOSEN_AHEAD: std::sync::Mutex<Option<(PathBuf, std::time::Instant)>> =
    std::sync::Mutex::new(None);

/// How long a path chosen ahead waits for its panel: one left unused does
/// not answer a person's panel later on.
const CHOSEN_AHEAD_FOR: std::time::Duration = std::time::Duration::from_secs(30);

/// The control port's `ui.choose {path}`: the next of EDM's panels
/// (`ask`), opened within half a minute, is answered with `path` as though
/// a person had chosen it there, rather than shown. A script cannot reach
/// the system's own panel (it is not EDM's window), and Import, Export and
/// the Browse… buttons are measured and checked through it (gaps row 183).
pub fn choose_ahead(path: PathBuf) {
    let chosen = Some((path, std::time::Instant::now()));
    #[cfg(test)]
    CHOSEN_HERE.with(|c| *c.borrow_mut() = chosen);
    #[cfg(not(test))]
    {
        *edm_core::lock(&CHOSEN_AHEAD) = chosen;
    }
}

/// The path chosen ahead, taken, where it is still fresh.
fn chosen_ahead() -> Option<PathBuf> {
    #[cfg(test)]
    let chosen = CHOSEN_HERE.with(|c| c.borrow_mut().take());
    #[cfg(not(test))]
    let chosen = edm_core::lock(&CHOSEN_AHEAD).take();
    chosen
        .filter(|(_, at)| at.elapsed() < CHOSEN_AHEAD_FOR)
        .map(|(path, _)| path)
}

/// Whether a path chosen ahead, still fresh, waits for the next panel:
/// Import and Export then ask the panel, which it answers, even where the
/// desktop has no chooser to show (`listfile::typed_instead`).
pub fn chosen_ahead_waits() -> bool {
    #[cfg(test)]
    let at = CHOSEN_HERE.with(|c| c.borrow().as_ref().map(|(_, at)| *at));
    #[cfg(not(test))]
    let at = edm_core::lock(&CHOSEN_AHEAD).as_ref().map(|(_, at)| *at);
    at.is_some_and(|at| at.elapsed() < CHOSEN_AHEAD_FOR)
}

#[cfg(test)]
thread_local! {
    /// In the window's tests, a test's own path chosen ahead: each runs its window on a thread
    /// of its own, beside the others, and one's Export took another's path.
    static CHOSEN_HERE: std::cell::RefCell<Option<(PathBuf, std::time::Instant)>> =
        const { std::cell::RefCell::new(None) };
}

/// Opens the system's own panel for `asked` and answers with what was
/// chosen once it is closed, without holding the window's thread while it
/// is open (gaps row 183): the window goes on drawing, the downloads'
/// figures moving and the control port answered. `rfd`'s panels held the
/// thread in a modal loop for as long as they were open. On macOS the
/// NSOpenPanel or NSSavePanel, on its own as before (`rfd`'s own way of
/// not waiting is a sheet on the window); elsewhere `rfd`'s, the portal's or
/// the common dialog.
pub fn ask(asked: Asked) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<PathBuf>>>> {
    if let Some(chosen) = chosen_ahead() {
        return Box::pin(async move { Some(chosen) });
    }
    #[cfg(target_os = "macos")]
    {
        let answer = macos::ask(&asked);
        Box::pin(async move { answer.await.flatten() })
    }
    // On a phone, without the window (Back up EDM, Open with…): a file to write goes to the
    // phone's save folder; nothing else is answered (`phone::ask` takes the window's).
    #[cfg(target_os = "android")]
    {
        let answer = asked
            .save
            .then(|| crate::platform::phone::save_place(&asked))
            .flatten();
        Box::pin(async move { answer })
    }
    #[cfg(not(any(target_os = "macos", target_os = "android")))]
    {
        let mut dialog = rfd::AsyncFileDialog::new();
        if let Some((name, extensions)) = asked.filter {
            dialog = dialog.add_filter(name, extensions);
        }
        if let Some(directory) = &asked.directory {
            dialog = dialog.set_directory(directory);
        }
        if let Some(name) = &asked.name {
            dialog = dialog.set_file_name(name);
        }
        Box::pin(async move {
            let chosen = if asked.folder {
                dialog.pick_folder().await
            } else if asked.save {
                dialog.save_file().await
            } else {
                dialog.pick_file().await
            };
            chosen.map(|file| file.path().to_path_buf())
        })
    }
}

/// The panels on macOS, begun with a completion handler.
#[cfg(target_os = "macos")]
mod macos {
    use std::cell::Cell;
    use std::path::PathBuf;

    use edm_shell::app::errands::{answer_later, Answer, Teller};
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponse, NSModalResponseOK, NSOpenPanel, NSSavePanel};
    use objc2_foundation::{NSArray, NSString, NSURL};

    use super::Asked;

    /// Opens the panel `asked` for and answers once it is closed: `None`
    /// for Cancel, or where it is asked off the main thread, where no panel
    /// can open.
    pub(crate) fn ask(asked: &Asked) -> Answer<Option<PathBuf>> {
        let (teller, answer) = answer_later();
        let Some(mtm) = MainThreadMarker::new() else {
            teller.tell(None);
            return answer;
        };
        if asked.save {
            let panel = NSSavePanel::savePanel(mtm);
            set_up(&panel, asked);
            if let Some(name) = &asked.name {
                panel.setNameFieldStringValue(&NSString::from_str(name));
            }
            let shown = panel.clone();
            begin(&panel, teller, move || shown.URL());
        } else {
            let panel = NSOpenPanel::openPanel(mtm);
            panel.setCanChooseFiles(!asked.folder);
            panel.setCanChooseDirectories(asked.folder);
            panel.setAllowsMultipleSelection(false);
            set_up(&panel, asked);
            let shown = panel.clone();
            begin(&panel, teller, move || shown.URLs().firstObject());
        }
        answer
    }

    /// What both panels take: a folder can be made where a folder is asked
    /// for, the kind of file, and where it opens.
    fn set_up(panel: &NSSavePanel, asked: &Asked) {
        if asked.folder || asked.save {
            panel.setCanCreateDirectories(true);
        }
        if let Some((_, extensions)) = asked.filter {
            let endings: Vec<_> = extensions.iter().map(|e| NSString::from_str(e)).collect();
            // The endings, as `rfd` gives them: the newer content types
            // need a type looked up for each.
            #[allow(deprecated)]
            panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&endings)));
        }
        if let Some(directory) = asked.directory.as_ref().and_then(|d| d.to_str()) {
            let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(directory), true);
            panel.setDirectoryURL(Some(&url));
        }
    }

    /// Shows `panel` on its own and tells `teller` the path of what
    /// `chosen` reads from it once OK closes it, none for Cancel.
    fn begin(
        panel: &NSSavePanel,
        teller: Teller<Option<PathBuf>>,
        chosen: impl Fn() -> Option<Retained<NSURL>> + 'static,
    ) {
        let teller = Cell::new(Some(teller));
        let handler = block2::RcBlock::new(move |response: NSModalResponse| {
            let Some(teller) = teller.take() else {
                return;
            };
            let path = (response == NSModalResponseOK)
                .then(&chosen)
                .flatten()
                .and_then(|url| url.path())
                .map(|path| PathBuf::from(path.to_string()));
            teller.tell(path);
        });
        // AppKit copies the handler and calls it once, on the main thread,
        // when the panel closes.
        panel.beginWithCompletionHandler(&handler);
    }
}

/// Asks the system's panel for `asked` and, where something is chosen, does
/// `then` with it in `cx`'s view once the panel closes (`ask`).
pub fn ask_then<T: 'static>(
    asked: Asked,
    cx: &mut gpui::Context<T>,
    then: impl FnOnce(&mut T, PathBuf, &mut gpui::Context<T>) + 'static,
) {
    #[cfg(not(target_os = "android"))]
    let chosen = ask(asked);
    #[cfg(target_os = "android")]
    let chosen = ask_on_phone(asked, None, cx);
    cx.spawn(async move |this, cx| {
        if let Some(path) = chosen.await {
            let _ = this.update(cx, |this, cx| {
                then(this, path, cx);
                cx.notify();
            });
        }
    })
    .detach();
}

/// [`ask_then`] for what needs the window as well: a field set to the
/// folder chosen.
pub fn ask_then_in<T: 'static>(
    asked: Asked,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<T>,
    then: impl FnOnce(&mut T, PathBuf, &mut gpui::Window, &mut gpui::Context<T>) + 'static,
) {
    #[cfg(not(target_os = "android"))]
    let chosen = ask(asked);
    #[cfg(target_os = "android")]
    let chosen = ask_on_phone(asked, Some(window.window_handle()), cx);
    cx.spawn_in(window, async move |this, cx| {
        if let Some(path) = chosen.await {
            let _ = this.update_in(cx, |this, window, cx| {
                then(this, path, window, cx);
                cx.notify();
            });
        }
    })
    .detach();
}

/// [`ask`] on a phone, where the window's own prompts answer (`phone::ask`); a path chosen
/// ahead first, as on a computer.
#[cfg(target_os = "android")]
fn ask_on_phone(
    asked: Asked,
    window: Option<gpui::AnyWindowHandle>,
    cx: &mut gpui::App,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<PathBuf>>>> {
    if let Some(chosen) = chosen_ahead() {
        return Box::pin(async move { Some(chosen) });
    }
    crate::platform::phone::ask(asked, window, cx)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    /// Only a named Wayland display sends EDM's copies through GPUI's
    /// clipboard (gaps row 218): an empty or missing one is X11's or none.
    #[test]
    fn a_named_wayland_display_is_wayland() {
        assert!(super::on_wayland(Some(OsStr::new("wayland-0"))));
        assert!(!super::on_wayland(Some(OsStr::new(""))));
        assert!(!super::on_wayland(None));
    }
}
