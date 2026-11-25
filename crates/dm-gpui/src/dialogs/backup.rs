// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! File › Back up dm… and Restore… (gaps row W16): the small dialog each opens, and the
//! window's part in making and putting back a backup (`dm_shell::backup`).
//!
//! Back up dm says what a backup holds, and that the sign-ins go in only with a passphrase,
//! typed twice; its Back up asks the system's save panel where (or takes the path typed, where
//! there is no file chooser, as Export does) and closes once the file is written. Restore reads
//! the backup chosen in the system's open panel, then says what restoring does and, where its
//! sign-ins are sealed, asks for the passphrase: left empty, everything else is restored
//! without them; a wrong one is said in the dialog, which stays open, and nothing is restored.
//! Its Restore puts the settings in force as Options' OK does, a download that is running going
//! on, and adds the downloads to the list, those it holds already left out.

use std::path::{Path, PathBuf};

use dm_desktop_base::i18n::{t, tf};
use dm_shell::backup::{Backup, Refused, Restored, SUGGESTED};
use dm_shell::settings::KeptPrograms;
use gpui::{
    prelude::*, size, App, Bounds, Context, Entity, FocusHandle, KeyDownEvent, SharedString,
    WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::dialogs::listfile::{self, Told};
use crate::platform::services::Asked;
use crate::widgets::native;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// The passphrase's field.
const PASSPHRASE: &str = "Passphrase";
/// The field the passphrase is typed in again.
const AGAIN: &str = "Passphrase again";
/// The field of the file's path, typed where there is no file chooser (Export's words).
const PATH: &str = "File's path";
/// The backup's file, beside its path.
const FROM: &str = "From";

/// What Back up dm says of a backup.
const BACKUP_SAYS: &str = "One file of dm's settings, queues, schedules, site rules, saved searches and list of downloads. The sign-ins (passwords, and the cookies a browser handed over) go in only with a passphrase, sealed with it.";
/// What Restore says it does.
const RESTORE_SAYS: &str = "Its settings, queues, schedules, site rules and saved searches replace these, as OK in Options does; its downloads are added to the list, those already in it left out. Downloads running go on.";
/// What Restore says of the passphrase.
const SEALED_SAYS: &str = "Where its sign-ins are sealed, type the passphrase; left empty, everything else is restored without them.";

/// The dialog's view: Back up dm's or Restore's.
pub struct BackupDialog {
    view: WeakEntity<Downloads>,
    /// Restore's, rather than Back up dm's.
    restore: bool,
    /// The backup Restore was opened over, chosen in the system's panel: its path, and whether
    /// its sign-ins are sealed. `None` where the path is typed (`path`) or not yet asked for.
    chosen: Option<(PathBuf, bool)>,
    /// The file's path, typed where there is no file chooser.
    path: Option<Entity<InputState>>,
    passphrase: Entity<InputState>,
    again: Entity<InputState>,
    says: Option<SharedString>,
    /// The file being written or read, or the save panel open: OK meanwhile is not taken.
    busy: bool,
    focus: FocusHandle,
    screen: Screen,
}

impl BackupDialog {
    /// Whether the passphrase is asked for: always by Back up dm; by Restore where the backup's
    /// sign-ins are sealed, or where it is not read yet (its path typed).
    fn asks_passphrase(&self) -> bool {
        !self.restore || self.chosen.as_ref().is_none_or(|(_, sealed)| *sealed)
    }

    /// OK (Back up, Restore): the two passphrases the same, then the file written or read, at
    /// the path typed, the one chosen, or (Back up dm) where the system's save panel says; the
    /// dialog closes once that is done, or says why not.
    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(view) = self.view.upgrade() else {
            window.remove_window();
            return;
        };
        let passphrase = self.passphrase.read(cx).value().to_string();
        let again = self.again.read(cx).value().to_string();
        if !self.restore && passphrase != again {
            return self.refused(t("The two passphrases differ.").to_string(), window, cx);
        }
        let path = match &self.path {
            Some(typed) => {
                let folder = view.read(cx).backend.settings().folders.download_dir;
                match listfile::typed_path(&typed.read(cx).value(), &folder.join(SUGGESTED)) {
                    Ok(path) => Some(path),
                    Err(why) => return self.refused(why, window, cx),
                }
            }
            None => self.chosen.as_ref().map(|(path, _)| path.clone()),
        };
        if let Some(path) = path {
            return self.go(path, passphrase, window, cx);
        }
        // Back up dm's file, asked of the system's save panel; the dialog stays open meanwhile,
        // and takes OK again once the panel is closed.
        self.busy = true;
        let chosen = crate::platform::services::ask(Asked::backup(true));
        cx.spawn_in(window, async move |this, cx| {
            let path = chosen.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                if let Some(path) = path {
                    this.go(path, passphrase, window, cx);
                }
            });
        })
        .detach();
    }

    /// The backup written to `path`, or read from it and restored.
    fn go(
        &mut self,
        path: PathBuf,
        passphrase: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.view.upgrade() else {
            window.remove_window();
            return;
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
        let restore = self.restore;
        view.update(cx, |view, cx| {
            if restore {
                view.restore_from(path, passphrase, told, cx);
            } else {
                view.back_up_to(path, passphrase, told, cx);
            }
        });
        cx.notify();
    }

    /// `why` said above the buttons, the passphrase (or the path) chosen to be put right.
    fn refused(&mut self, why: String, window: &mut Window, cx: &mut Context<Self>) {
        self.says = Some(why.into());
        let field = match &self.path {
            Some(path) if !self.asks_passphrase() => path.clone(),
            _ => self.passphrase.clone(),
        };
        field.update(cx, |p, cx| {
            p.focus(window, cx);
            p.select_all(window, cx);
        });
        cx.notify();
    }

    /// Cancel, Escape or the close button: nothing written or restored.
    fn cancel(&mut self, window: &mut Window) {
        self.busy = true;
        window.remove_window();
    }

    /// The form's rows, between what the dialog says and its buttons.
    fn fields(&self, theme: Theme, width: f32, screen: &Screen) -> Vec<gpui::AnyElement> {
        let mut rows = Vec::new();
        if let Some(path) = &self.path {
            rows.push(native::row(
                None,
                width,
                screen,
                native::note(t(listfile::NO_CHOOSER), theme.text, screen),
            ));
            rows.push(native::row(
                Some(t(PATH)),
                width,
                screen,
                native::field(path, PATH, theme),
            ));
        }
        if let Some((path, _)) = &self.chosen {
            rows.push(native::row(
                Some(t(FROM)),
                width,
                screen,
                screen.label(path.display().to_string()),
            ));
        }
        if self.restore && self.asks_passphrase() {
            rows.push(native::row(
                None,
                width,
                screen,
                native::note(t(SEALED_SAYS), theme.text, screen),
            ));
        }
        if self.asks_passphrase() {
            rows.push(native::row(
                Some(t(PASSPHRASE)),
                width,
                screen,
                native::field(&self.passphrase, PASSPHRASE, theme),
            ));
        }
        if !self.restore {
            rows.push(native::row(
                Some(t(AGAIN)),
                width,
                screen,
                native::field(&self.again, AGAIN, theme),
            ));
        }
        rows.into_iter()
            .map(IntoElement::into_any_element)
            .collect()
    }
}

impl Render for BackupDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin_in(window);
        let screen = self.screen.clone();
        let width = native::label_width(&[t(PASSPHRASE), t(AGAIN), t(PATH), t(FROM)]);
        let said = if self.restore {
            t(RESTORE_SAYS)
        } else {
            t(BACKUP_SAYS)
        };
        let go = if self.restore {
            t("Restore")
        } else {
            t("Back up")
        };
        let root = native::dialog("backup", title(self.restore), theme, cx)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter"
                    && crate::widgets::native::return_is_default(window, Some(&this.focus), cx)
                {
                    this.ok(window, cx);
                }
            }))
            .child(crate::widgets::fit::fitted(
                native::body(theme)
                    .child(native::row(
                        None,
                        width,
                        &screen,
                        native::note(said, theme.text, &screen),
                    ))
                    .children(self.fields(theme, width, &screen))
                    .child(native::row(
                        None,
                        width,
                        &screen,
                        native::says("backup-says", self.says.clone(), theme, &screen),
                    ))
                    .child(native::buttons(
                        Vec::new(),
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            native::button("ok", go.into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx)))
                                .into_any_element(),
                        ],
                    )),
            ));
        crate::window::safe::kept_clear(root, window)
    }
}

/// The dialog's title: Back up dm's or Restore's.
fn title(restore: bool) -> &'static str {
    if restore {
        t("Restore dm")
    } else {
        t("Back up dm")
    }
}

/// Opens Back up dm's dialog (`restore` false) or Restore's over the middle of the screen:
/// Restore's over the backup `chosen` (its path, and whether its sign-ins are sealed), or with
/// the file's path to type where none was chosen; Back up dm's with the path to type where
/// there is no file chooser.
pub(crate) fn open(
    view: WeakEntity<Downloads>,
    restore: bool,
    chosen: Option<(PathBuf, bool)>,
    cx: &mut App,
) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, restore, chosen, cx));
}

fn open_now(
    view: WeakEntity<Downloads>,
    restore: bool,
    chosen: Option<(PathBuf, bool)>,
    cx: &mut App,
) {
    // Asked for again while open: the one open, as it stands.
    if crate::window::screen::bring_named(title(restore), cx) {
        return;
    }
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let backend = &downloads.read(cx).backend;
    let typed = if restore {
        chosen.is_none()
    } else {
        listfile::typed_instead(
            backend.platform.can_choose_file(),
            crate::platform::services::chosen_ahead_waits(),
        )
    };
    let offered = (typed && !restore)
        .then(|| backend.settings().folders.download_dir.join(SUGGESTED))
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    // A sentence, two fields and a line, as wide as Add URL; more with a path to type.
    let (width, _) = native::WindowSize::AddUrl.here();
    let height = if typed { 400.0 } else { 330.0 };
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    let opened = crate::window::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(title(restore).into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let passphrase = cx.new(|cx| InputState::new(window, cx).masked(true));
            let again = cx.new(|cx| InputState::new(window, cx).masked(true));
            let path = typed.then(|| {
                cx.new(|cx| {
                    let mut p = InputState::new(window, cx);
                    p.set_value(offered, window, cx);
                    p
                })
            });
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let dialog = cx.new(|cx| {
                // Return in a field is OK; what was said goes once a field is changed.
                for field in [Some(&passphrase), Some(&again), path.as_ref()]
                    .into_iter()
                    .flatten()
                {
                    cx.subscribe_in(
                        field,
                        window,
                        |this: &mut BackupDialog, _, event, window, cx| match event {
                            InputEvent::PressEnter { .. } => this.ok(window, cx),
                            InputEvent::Change if this.says.take().is_some() => cx.notify(),
                            _ => {}
                        },
                    )
                    .detach();
                }
                BackupDialog {
                    view,
                    restore,
                    chosen,
                    path: path.clone(),
                    passphrase: passphrase.clone(),
                    again,
                    says: None,
                    busy: false,
                    focus: cx.focus_handle(),
                    screen,
                }
            });
            // The keys to the first field shown, or to the dialog where it has none.
            let first = path.or_else(|| dialog.read(cx).asks_passphrase().then_some(passphrase));
            match first {
                Some(field) => field.update(cx, |p, cx| {
                    p.focus(window, cx);
                    p.select_all(window, cx);
                }),
                None => {
                    let focus = dialog.read(cx).focus.clone();
                    window.focus(&focus, cx);
                }
            }
            native::close_is_cancel(&dialog, window, cx);
            cx.new(|cx| gpui_component::Root::new(dialog, window, cx))
        },
    );
    if let Err(e) = opened {
        dm_core::note!(
            error,
            "dm-gpui: {} could not be opened: {e}",
            title(restore)
        );
    }
}

impl native::CloseButton for BackupDialog {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}

/// Why a backup was not read or opened, worded on the window's thread in the language in force.
enum Failed {
    /// The file could not be read.
    Io(std::io::Error),
    /// It is not a backup, or its sign-ins did not open.
    Refused(Refused),
}

/// The backup at `path`, read; with `passphrase` (not empty), its sign-ins opened.
fn read_backup(path: &Path, passphrase: &str) -> Result<Backup, Failed> {
    let text = std::fs::read_to_string(path).map_err(Failed::Io)?;
    let mut backup = dm_shell::backup::read(&text).map_err(Failed::Refused)?;
    if !passphrase.is_empty() {
        backup.open(passphrase).map_err(Failed::Refused)?;
    }
    Ok(backup)
}

/// What `failed` says, of the file at `path`.
fn worded(failed: &Failed, path: &Path) -> String {
    match failed {
        Failed::Io(e) => tf(
            "{} couldn't be read: {}",
            &[&path.display(), &dm_desktop_base::format::file_error(e)],
        ),
        Failed::Refused(Refused::NotABackup) => t("That file is not a backup of dm.").to_string(),
        Failed::Refused(Refused::WrongPassphrase) => {
            t("That passphrase does not open this backup's sign-ins.").to_string()
        }
        Failed::Refused(Refused::SealedByNewer) => {
            t("This backup's sign-ins were sealed by a newer dm, which this one can't open.")
                .to_string()
        }
    }
}

/// What a restore from `path` did, said in the status bar: the downloads it added, those it
/// left out as the list holds them already, and what it did not restore.
fn restored_words(
    path: &Path,
    done: &Restored,
    kept: KeptPrograms,
    said: &[String],
    unread: usize,
) -> String {
    let added = done.added.len();
    let mut words = vec![tf(
        "Restored dm from {}. Downloads added to the list: {}.",
        &[&path.display(), &added],
    )];
    if done.already > 0 {
        words.push(tf(
            "Downloads already in the list, left out: {}.",
            &[&done.already],
        ));
    }
    if kept.command || kept.schedule || kept.proxy_script {
        words.push(t("The programs dm runs (after a download, at the end of the scheduled hours, the proxy script) were not restored: set them in Options.").to_string());
    }
    if !said.is_empty() {
        words.push(tf(
            "Some of its settings couldn't be read, so their defaults are in use: {}.",
            &[&said.join("; ")],
        ));
    }
    if unread > 0 {
        words.push(tf(
            "Downloads it holds that this dm can't read, left out: {}.",
            &[&unread],
        ));
    }
    words.join(" ")
}

impl Downloads {
    /// File › Back up dm…: its dialog.
    pub(crate) fn back_up_dm(&mut self, cx: &mut Context<Self>) {
        open(cx.entity().downgrade(), false, None, cx);
    }

    /// File › Restore…: the backup chosen in the system's open panel, read, and Restore's dialog
    /// opened over it; where there is no file chooser, the dialog takes the path typed.
    pub(crate) fn restore_dm(&mut self, cx: &mut Context<Self>) {
        let typed = listfile::typed_instead(
            self.backend.platform.can_choose_file(),
            crate::platform::services::chosen_ahead_waits(),
        );
        if typed {
            open(cx.entity().downgrade(), true, None, cx);
            return;
        }
        crate::platform::services::ask_then(
            Asked::backup(false),
            cx,
            |view: &mut Self, path, cx| view.restore_chosen(path, cx),
        );
    }

    /// The backup chosen at `path`, read off the window's thread, and Restore's dialog opened
    /// over it; or why it cannot be, said.
    fn restore_chosen(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.later(
            move |_| {
                let read = read_backup(&path, "");
                (path, read)
            },
            |view, (path, read), cx| match read {
                Ok(backup) => {
                    let chosen = Some((path, backup.sealed()));
                    open(cx.entity().downgrade(), true, chosen, cx);
                }
                Err(failed) => view.say(worded(&failed, &path)),
            },
            cx,
        );
    }

    /// Back up dm's file written to `path` off the window's thread, its sign-ins sealed with
    /// `passphrase` where one is given, and said once it is; whether it could be is `told`.
    pub(crate) fn back_up_to(
        &mut self,
        path: PathBuf,
        passphrase: String,
        told: Told,
        cx: &mut Context<Self>,
    ) {
        let sealed = !passphrase.is_empty();
        self.later(
            move |backend| {
                let written = backend
                    .back_up(Some(passphrase.as_str()))
                    .map(|text| dm_core::write_atomic(&path, text.as_bytes()));
                (path, written)
            },
            move |view, (path, written), cx| {
                let outcome = match written {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(e)) => Err(tf(
                        "{} couldn't be written: {}",
                        &[&path.display(), &dm_desktop_base::format::file_error(&e)],
                    )),
                    Err(why) => Err(why.into_string()),
                };
                match outcome {
                    Ok(()) => {
                        dm_desktop_base::phone::wrote(&path);
                        view.say(if sealed {
                            tf(
                                "Backed up dm to {}, its sign-ins sealed with the passphrase.",
                                &[&path.display()],
                            )
                        } else {
                            tf("Backed up dm to {}.", &[&path.display()])
                        });
                        told(Ok(()), cx);
                    }
                    Err(why) => {
                        view.say(why.clone());
                        told(Err(why), cx);
                    }
                }
            },
            cx,
        );
    }

    /// Restore's: the backup at `path` read off the window's thread, its sign-ins opened with
    /// `passphrase` where one is given, then restored (`restore_backup`). A file that cannot be
    /// read, is not a backup, or whose sign-ins the passphrase does not open is `told`, and
    /// nothing is restored.
    pub(crate) fn restore_from(
        &mut self,
        path: PathBuf,
        passphrase: String,
        told: Told,
        cx: &mut Context<Self>,
    ) {
        self.later(
            move |_| {
                let read = read_backup(&path, &passphrase);
                (path, read)
            },
            move |view, (path, read), cx| match read {
                Ok(backup) => view.restore_backup(path, backup, told, cx),
                Err(failed) => told(Err(worded(&failed, &path)), cx),
            },
            cx,
        );
    }

    /// `backup` put back: its settings in force as Options' OK puts them (this computer's
    /// programs kept), then its downloads added off the window's thread and what was done said.
    fn restore_backup(
        &mut self,
        path: PathBuf,
        backup: Backup,
        told: Told,
        cx: &mut Context<Self>,
    ) {
        let mine = self.backend.settings();
        let (wanted, kept, said) = match backup.settings_over(&mine) {
            Ok(over) => over,
            Err(why) => return told(Err(why.into_string()), cx),
        };
        if let Err(why) = self.apply_settings(wanted, cx) {
            return told(Err(why), cx);
        }
        told(Ok(()), cx);
        // The rows are the window's own, as an imported list's are: no progress windows.
        let adding = self.session.adding();
        self.later(
            move |backend| {
                let done = backend.restore_rows(&backup);
                adding.added(&done.added);
                (done, backup.unread_rows)
            },
            move |view, (done, unread), _| {
                view.say(restored_words(&path, &done, kept, &said, unread));
                view.read();
            },
            cx,
        );
    }
}
