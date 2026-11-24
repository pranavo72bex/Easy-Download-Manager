// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The main window's commands that act on files and lists: open, preview,
//! show in Finder and extract a download's file;
//! import a list (asked first where some of it is listed already) and
//! export the list, the file typed where there is no chooser; select every
//! row shown; Help. Each rule is the application's; the window says what
//! came of it.

use dm_core::Entry;
use dm_desktop_base::i18n::{t, tf, tn};
use gpui::Context;

use crate::dialogs::listfile::{untold, Told};
use crate::window::list::Downloads;

impl Downloads {
    /// Open, Open with…, Preview, Show in Finder, Extract here or Extract
    /// to…, for the first row chosen.
    pub(crate) fn act_on_file(
        &mut self,
        command: dm_shell::app::commands::Command,
        chosen: &[Entry],
        cx: &mut Context<Self>,
    ) {
        use dm_shell::app::commands::Command;
        let Some(row) = chosen.first() else {
            return;
        };
        // Open, Open with… and Preview run the file: a person's click does,
        // a script's a moment ago does not (gaps row 159 f).
        if matches!(
            command,
            Command::OpenFile | Command::OpenWith | Command::Preview
        ) {
            if let Some(why) = self.port.scripted.run_refused() {
                self.say(why.to_string());
                cx.notify();
                return;
            }
        }
        match command {
            Command::OpenFile => self.open_path(&row.path),
            Command::OpenWith => self.open_path_with(&row.path, |_, _| {}, cx),
            Command::OpenFolder => self.reveal_path(&row.path),
            // Off the window's thread: where the disk takes no hard link the
            // preview is a copy of what is fetched so far.
            Command::Preview => {
                let id = row.id;
                self.later(
                    move |backend| backend.preview(id),
                    |view, made, _| match made {
                        Ok(path) => view.open_path(&path.to_string_lossy()),
                        // Nothing on disk yet is the usual answer, and one to word.
                        Err(e) if e.starts_with("nothing") => {
                            view.say(t("Nothing of it is on disk to preview yet.").into())
                        }
                        Err(e) => view.say(e),
                    },
                    cx,
                );
            }
            Command::ExtractHere => self.extract(&row.path, &row.url, None),
            Command::ExtractTo => {
                let (path, url) = (row.path.clone(), row.url.clone());
                crate::platform::services::ask_then(
                    crate::platform::services::Asked::folder(),
                    cx,
                    move |view: &mut Self, folder, _| view.extract(&path, &url, Some(folder)),
                );
            }
            _ => {}
        }
        cx.notify();
    }

    /// Extracts a finished ZIP into a new folder beside it (or `into`), off
    /// the window's thread; the answer is said as an opener's is. Nothing in
    /// it is opened or run.
    fn extract(&mut self, path: &str, url: &str, into: Option<std::path::PathBuf>) {
        let said = self.opener_said();
        self.say(t("Extracting…").into());
        // The answer is worded on the extracting thread, which has no
        // language of its own: it is given the window's.
        let language = dm_desktop_base::i18n::code();
        let started = dm_shell::app::files::extract_later(
            std::path::PathBuf::from(path),
            url.to_string(),
            into,
            move |answer| {
                dm_desktop_base::i18n::set(language);
                let answer = match answer {
                    Ok(done) => tn(
                        "Extracted {} file to {}||Extracted {} files to {}",
                        done.files as u64,
                        &[&done.files, &done.folder.display()],
                    ),
                    Err(e) => tf("Not extracted: {}", &[&e]),
                };
                *dm_core::lock(&said) = Some(answer);
            },
        );
        if let Err(e) = started {
            self.say(tf("Not extracted: {}", &[&e]));
        }
    }

    /// Select all: every row the list shows.
    pub(crate) fn select_all(&mut self, cx: &mut Context<Self>) {
        self.list.selected = self.list.shown.iter().map(|&i| self.rows[i].id).collect();
        cx.notify();
    }

    /// Help: dm's own page, in the browser.
    pub(crate) fn help(&mut self) {
        if let Err(e) = self.backend.platform.open(dm_desktop_base::project::HELP) {
            self.say(e);
        }
    }

    /// Import: a list chosen in the system's open panel, added (a .torrent
    /// is a download, not a list); where some of its addresses are listed
    /// already, asked first whether to skip them or add them again. Where
    /// the desktop has no file chooser, the path is typed instead
    /// (`listfile`, gaps row U41).
    pub(crate) fn import(&mut self, cx: &mut Context<Self>) {
        if self.typed_list_path(false, cx) {
            return;
        }
        crate::platform::services::ask_then(
            crate::platform::services::Asked::file(false, ""),
            cx,
            |view: &mut Self, path, cx| view.import_from(&path, untold(), cx),
        );
    }

    /// Opens the dialog that takes Import's or Export's (`save`) file as a
    /// typed path, where the desktop has no file chooser to ask with: both
    /// did nothing there. False where the panel is to be asked.
    fn typed_list_path(&mut self, save: bool, cx: &mut Context<Self>) -> bool {
        let typed = crate::dialogs::listfile::typed_instead(
            self.backend.platform.can_choose_file(),
            crate::platform::services::chosen_ahead_waits(),
        );
        if typed {
            crate::dialogs::listfile::open(cx.entity().downgrade(), save, cx);
        }
        typed
    }

    /// Import of the list at `path`, chosen in the panel or typed. The list
    /// is read, and what of it is listed already counted, off the window's
    /// thread (gaps row 183); whether it could be is `told`.
    pub(crate) fn import_from(
        &mut self,
        path: &std::path::Path,
        told: Told,
        cx: &mut Context<Self>,
    ) {
        if let Some(local) = dm_desktop_base::add_url::local_torrent(&path.to_string_lossy()) {
            let items = vec![(local, dm_core::AddOptions::default())];
            match self.add_items(items, cx) {
                Ok(_) => told(Ok(()), cx),
                Err(e) => {
                    self.say(e.clone());
                    told(Err(e), cx);
                }
            }
            return;
        }
        let rows = self.rows.clone();
        let path = path.to_path_buf();
        self.later(
            move |_| {
                let read = std::fs::read_to_string(&path).map(|text| {
                    let (already, total) = dm_shell::app::import::already_listed(&rows, &text);
                    (text, already, total)
                });
                (path, read)
            },
            |view, (path, read), cx| match read {
                Ok((text, already, total)) => {
                    told(Ok(()), cx);
                    view.import_read(text, already, total, cx);
                }
                Err(e) => {
                    let why = tf(
                        "{} couldn't be read: {}",
                        &[&path.display(), &dm_desktop_base::format::file_error(&e)],
                    );
                    view.say(why.clone());
                    told(Err(why), cx);
                }
            },
            cx,
        );
    }

    /// A list read for Import: added, or where `already` of its `total`
    /// addresses are listed, asked first whether to skip them.
    fn import_read(&mut self, text: String, already: usize, total: usize, cx: &mut Context<Self>) {
        if already == 0 {
            self.import_text(text, false, cx);
            return;
        }
        let asked = crate::dialogs::question::Asked {
            title: t("Some are already in the list").into(),
            lines: vec![tn(
                "{} of the {} addresses in this list is already in dm's list.||{} of the {} addresses in this list are already in dm's list.",
                already as u64,
                &[&already, &total],
            )
            .into()],
            named: None,
            detail: None,
            toggle: None,
            ticked: false,
            // Skip them (the default), Add them again beside, Cancel.
            buttons: vec![
                t("Skip them").into(),
                t("Add them again").into(),
                t("Cancel").into(),
            ],
            default: 0,
            escape: 2,
            destroys: None,
        };
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, _, cx| {
            if button == 2 {
                return;
            }
            let _ = view.update(cx, |view, cx| view.import_text(text, button == 0, cx));
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// Adds the downloads a list holds; `skip_listed`, not those whose
    /// address is already a row; and says how many, and what was not read.
    /// Added off the window's thread: ten thousand rows held it for
    /// seconds (gaps row 183).
    fn import_text(&mut self, text: String, skip_listed: bool, cx: &mut Context<Self>) {
        let rows = self.rows.clone();
        // A list is not a run of progress windows: its rows are the
        // window's own, held unjudged by the reads made while it is added
        // and marked so once their ids are known
        // (`docs/decisions.md`, 2026-11-12).
        let adding = self.session.adding();
        self.later(
            move |backend| {
                let done = dm_shell::app::import::import(backend, &rows, &text, skip_listed);
                adding.added(&done.new);
                done
            },
            |view, done, _| {
                let (added, unread) = (done.added, done.unread);
                view.say(if unread > 0 {
                    tf(
                        "Imported {}. Skipped {} lines with no link in them (a link starts with http://, https://, ftp:// or magnet:).",
                        &[&added, &unread],
                    )
                } else {
                    tn(
                        "Imported {} download.||Imported {} downloads.",
                        added as u64,
                        &[&added],
                    )
                });
                view.read();
            },
            cx,
        );
    }

    /// Export: the list written where the system's save panel says, or
    /// where a path typed says where the desktop has no file chooser.
    pub(crate) fn export(&mut self, cx: &mut Context<Self>) {
        if self.typed_list_path(true, cx) {
            return;
        }
        crate::platform::services::ask_then(
            crate::platform::services::Asked::file(true, "downloads.dmlist"),
            cx,
            |view: &mut Self, path, cx| view.export_to(path, untold(), cx),
        );
    }

    /// The list written to `path`, off the window's thread (a long list is
    /// a long file), and said once it is; whether it could be is `told`.
    pub(crate) fn export_to(
        &mut self,
        path: std::path::PathBuf,
        told: Told,
        cx: &mut Context<Self>,
    ) {
        self.later(
            move |backend| {
                let written = std::fs::write(&path, backend.export());
                (path, written)
            },
            |view, (path, written), cx| match written {
                Ok(()) => {
                    dm_desktop_base::phone::wrote(&path);
                    view.say(tf("Exported to {}.", &[&path.display()]));
                    told(Ok(()), cx);
                }
                Err(e) => {
                    let why = tf(
                        "{} couldn't be written: {}",
                        &[&path.display(), &dm_desktop_base::format::file_error(&e)],
                    );
                    view.say(why.clone());
                    told(Err(why), cx);
                }
            },
            cx,
        );
    }
}
