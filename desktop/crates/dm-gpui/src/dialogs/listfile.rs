// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Import's or Export's file, its path typed, where the desktop has no
//! file chooser to ask with (a Linux desktop with neither the portal's nor
//! zenity's): both did nothing there, without a word (gaps row U41). It
//! says "No file picker here: type the path.", and has the file's path
//! (Export's the download folder's `downloads.dmlist`), OK and Cancel. A
//! path that is not a full one, or a list that cannot be read or written,
//! is said in it, and it stays open with the path as typed, to be put
//! right.

use std::path::{Path, PathBuf};

use dm_desktop_base::i18n::{t, tf};
use gpui::{
    div, prelude::*, size, App, Bounds, Context, Entity, KeyDownEvent, SharedString, WeakEntity,
    Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::widgets::native;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// What the dialog says of itself, as the folder fields' Browse… says its
/// own (`savefolder::NO_CHOOSER`).
pub const NO_CHOOSER: &str = "No file picker here: type the path.";

/// The field's name.
const PATH: &str = "File's path";

/// What came of an Import or an Export started here, told back once the
/// file is read or written: the dialog closes, or says why not.
pub(crate) type Told = Box<dyn FnOnce(Result<(), String>, &mut App)>;

/// A [`Told`] that tells no one: the system's panel chose the file.
pub(crate) fn untold() -> Told {
    Box::new(|_, _| {})
}

/// Whether Import or Export asks for the path typed: there is no file
/// chooser here (`chooser`), and no script chose a file ahead
/// (`ui.choose`), which answers the panel that would have opened.
pub(crate) fn typed_instead(chooser: bool, chosen_ahead: bool) -> bool {
    !chooser && !chosen_ahead
}

/// Where Export offers to write the list: the download folder's
/// `downloads.dmlist`.
pub(crate) fn suggested(download_dir: &Path) -> PathBuf {
    download_dir.join("downloads.dmlist")
}

/// The file a typed path names (`~` the home folder), or what to say: a
/// full path is wanted, such as `example`.
pub(crate) fn typed_path(typed: &str, example: &Path) -> Result<PathBuf, String> {
    dm_shell::settings::full_path(typed).ok_or_else(|| {
        tf(
            "Type the file's full path, such as {}.",
            &[&example.display()],
        )
    })
}

/// The dialog's view.
pub struct ListFile {
    view: WeakEntity<Downloads>,
    /// Export's, rather than Import's.
    save: bool,
    path: Entity<InputState>,
    says: Option<SharedString>,
    /// The file being read or written: OK meanwhile is not taken.
    busy: bool,
    screen: Screen,
}

impl ListFile {
    /// OK: the path typed read, and the list imported from it or exported
    /// to it; the dialog closes once that is done, or says why not.
    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(view) = self.view.upgrade() else {
            window.remove_window();
            return;
        };
        let example = suggested(&view.read(cx).backend.settings().download_dir);
        let path = match typed_path(&self.path.read(cx).value(), &example) {
            Ok(path) => path,
            Err(why) => return self.refused(why, window, cx),
        };
        self.busy = true;
        let (me, dialog) = (cx.entity().downgrade(), window.window_handle());
        let told: Told = Box::new(move |done, cx| {
            cx.defer(move |cx| {
                let _ = dialog.update(cx, |_, window, cx| match done {
                    Ok(()) => window.remove_window(),
                    Err(why) => {
                        let _ = me.update(cx, |me, cx| {
                            me.busy = false;
                            me.refused(why, window, cx);
                        });
                    }
                });
            });
        });
        let save = self.save;
        view.update(cx, |view, cx| {
            if save {
                view.export_to(path, told, cx);
            } else {
                view.import_from(&path, told, cx);
            }
        });
    }

    /// `why` said under the path, which keeps the keyboard, chosen, to be
    /// put right: closed, the path was lost and typing went to the list.
    fn refused(&mut self, why: String, window: &mut Window, cx: &mut Context<Self>) {
        self.says = Some(why.into());
        self.path.update(cx, |p, cx| {
            p.focus(window, cx);
            p.select_all(window, cx);
        });
        cx.notify();
    }

    /// Cancel, Escape or the close button: nothing read or written.
    fn cancel(&mut self, window: &mut Window) {
        self.busy = true;
        window.remove_window();
    }
}

impl Render for ListFile {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let width = native::label_width(&[t(PATH)]);
        native::dialog("list-file", title(self.save), theme, cx)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter" {
                    this.ok(window, cx);
                }
            }))
            .child(crate::widgets::fit::fitted(
                native::body(theme)
                    .child(native::row(
                        None,
                        width,
                        &screen,
                        native::note(t(NO_CHOOSER), theme.text, &screen),
                    ))
                    .child(native::row(
                        Some(t(PATH)),
                        width,
                        &screen,
                        native::field(&self.path, PATH, theme),
                    ))
                    .child(native::row(
                        None,
                        width,
                        &screen,
                        div().children(
                            self.says
                                .clone()
                                .map(|says| native::note(says, theme.warning, &screen)),
                        ),
                    ))
                    .child(native::buttons(
                        Vec::new(),
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            native::button("ok", t("OK").into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx)))
                                .into_any_element(),
                        ],
                    )),
            ))
    }
}

/// The dialog's title: Export's or Import's.
fn title(save: bool) -> &'static str {
    if save {
        t("Export the list")
    } else {
        t("Import a list")
    }
}

/// Opens the dialog over the middle of the screen: Export's (`save`) with
/// the download folder's `downloads.dmlist` in its field, Import's empty.
pub fn open(view: WeakEntity<Downloads>, save: bool, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, save, cx));
}

fn open_now(view: WeakEntity<Downloads>, save: bool, cx: &mut App) {
    // Asked for again while open: the one open, as it stands.
    if crate::window::screen::bring_named(title(save), cx) {
        return;
    }
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let offered = save
        .then(|| suggested(&downloads.read(cx).backend.settings().download_dir))
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    // One field and a line, as Add URL's.
    let (width, height) = native::WindowSize::AddUrl.here();
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    let opened = crate::window::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(title(save).into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let path = cx.new(|cx| {
                let mut p = InputState::new(window, cx);
                p.set_value(offered, window, cx);
                p
            });
            path.update(cx, |p, cx| {
                p.focus(window, cx);
                p.select_all(window, cx);
            });
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let dialog = cx.new(|cx| {
                // Return in the field is OK; what was said goes once the
                // path is changed.
                cx.subscribe_in(
                    &path,
                    window,
                    |this: &mut ListFile, _, event, window, cx| match event {
                        InputEvent::PressEnter { .. } => this.ok(window, cx),
                        InputEvent::Change if this.says.take().is_some() => cx.notify(),
                        _ => {}
                    },
                )
                .detach();
                ListFile {
                    view,
                    save,
                    path: path.clone(),
                    says: None,
                    busy: false,
                    screen,
                }
            });
            native::close_is_cancel(&dialog, window, cx);
            cx.new(|cx| gpui_component::Root::new(dialog, window, cx))
        },
    );
    if let Err(e) = opened {
        dm_core::note!(error, "dm-gpui: {} could not be opened: {e}", title(save));
    }
}

impl native::CloseButton for ListFile {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}

// After the window's own code: the catalogs' check reads a file's words up to its first
// test-only item (`i18n_check`).
#[cfg(test)]
impl ListFile {
    /// The path as it stands in the field, for the window's tests.
    pub(crate) fn typed(&self, cx: &App) -> String {
        self.path.read(cx).value().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The path is asked for typed only where there is no chooser and no
    /// script chose a file ahead.
    #[test]
    fn a_typed_path_is_asked_for_only_where_no_chooser_is() {
        assert!(typed_instead(false, false));
        assert!(!typed_instead(true, false));
        assert!(!typed_instead(false, true));
        assert!(!typed_instead(true, true));
    }

    /// Export offers the download folder's `downloads.dmlist`.
    #[test]
    fn export_offers_the_download_folders_list() {
        let folder = std::env::temp_dir().join("dm-out");
        assert_eq!(suggested(&folder), folder.join("downloads.dmlist"));
    }

    /// A full path is taken; an empty or a relative one is refused with an
    /// example; `~` is the home folder.
    #[test]
    fn only_a_full_path_is_taken() {
        let example = std::env::temp_dir().join("downloads.dmlist");
        let full = std::env::temp_dir().join("typed.dmlist");
        assert_eq!(
            typed_path(&format!("  {}  ", full.display()), &example),
            Ok(full)
        );
        for typed in ["", "   ", "relative.dmlist", "lists/typed.dmlist"] {
            let why = typed_path(typed, &example).unwrap_err();
            assert!(why.contains("full path"), "{typed:?}: {why}");
            assert!(why.contains(&example.display().to_string()), "{why}");
        }
        if let Some(home) = dm_core::folders::home() {
            assert_eq!(
                typed_path("~/typed.dmlist", &example),
                Ok(home.join("typed.dmlist"))
            );
        }
    }
}
