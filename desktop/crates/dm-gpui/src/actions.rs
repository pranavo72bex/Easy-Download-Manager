// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The main window's commands that act on files and lists, as the egui
//! window's do: open, preview, show in Finder and extract a download's file;
//! import a list (asked first where some of it is listed already) and
//! export the list; select every row shown; Help. Each rule is the
//! application's; the window says what came of it.

use dm_core::Entry;
use dm_desktop_base::i18n::{t, tf, tn};
use gpui::Context;

use crate::list::Downloads;

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
            if let Some(why) = self.scripted.run_refused() {
                self.say(why.to_string());
                cx.notify();
                return;
            }
        }
        match command {
            Command::OpenFile => self.open_path(&row.path),
            Command::OpenWith => self.open_path_with(&row.path, |_, _| {}, cx),
            Command::OpenFolder => self.reveal_path(&row.path),
            Command::Preview => match self.backend.preview(row.id) {
                Ok(path) => self.open_path(&path.to_string_lossy()),
                // Nothing on disk yet is the usual answer, and one to word.
                Err(e) if e.starts_with("nothing") => {
                    self.say(t("Nothing of it is on disk to preview yet.").into())
                }
                Err(e) => self.say(e),
            },
            Command::ExtractHere => self.extract(&row.path, &row.url, None),
            Command::ExtractTo => {
                let (path, url) = (row.path.clone(), row.url.clone());
                crate::services::ask_then(
                    crate::services::Asked::folder(),
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
        let started = dm_shell::app::files::extract_later(
            std::path::PathBuf::from(path),
            url.to_string(),
            into,
            move |answer| {
                let answer = match answer {
                    Ok(done) => format!(
                        "Extracted {} {} to {}",
                        done.files,
                        if done.files == 1 { "file" } else { "files" },
                        done.folder.display()
                    ),
                    Err(e) => format!("Not extracted: {e}"),
                };
                *dm_core::lock(&said) = Some(answer);
            },
        );
        if let Err(e) = started {
            self.say(format!("Not extracted: {e}"));
        }
    }

    /// Select all: every row the list shows.
    pub(crate) fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selected = self.shown.iter().map(|&i| self.rows[i].id).collect();
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
    /// already, asked first whether to skip them or add them again.
    pub(crate) fn import(&mut self, cx: &mut Context<Self>) {
        crate::services::ask_then(
            crate::services::Asked::file(false, ""),
            cx,
            |view: &mut Self, path, cx| view.import_from(&path, cx),
        );
    }

    /// Import of the list at `path`, chosen in the panel.
    fn import_from(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        if let Some(local) = dm_desktop_base::add_url::local_torrent(&path.to_string_lossy()) {
            let items = vec![(local, dm_core::AddOptions::default())];
            if let Err(e) = self.add_items(items, cx) {
                self.say(e);
            }
            return;
        }
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                self.say(tf(
                    "{} could not be read: {}",
                    &[&path.display(), &dm_desktop_base::format::file_error(&e)],
                ));
                return;
            }
        };
        let (already, total) = dm_shell::app::import::already_listed(&self.rows, &text);
        if already == 0 {
            self.import_text(&text, false, cx);
            return;
        }
        let asked = crate::question::Asked {
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
            // Skip them (the default), Add them again beside, Cancel.
            buttons: vec![
                t("Skip them").into(),
                t("Add them again").into(),
                t("Cancel").into(),
            ],
            default: 0,
            escape: 2,
        };
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, _, cx| {
            if button == 2 {
                return;
            }
            let _ = view.update(cx, |view, cx| view.import_text(&text, button == 0, cx));
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// Adds the downloads a list holds; `skip_listed`, not those whose
    /// address is already a row; and says how many, and what was not read.
    fn import_text(&mut self, text: &str, skip_listed: bool, cx: &mut Context<Self>) {
        let done = dm_shell::app::import::import(&self.backend, &self.rows, text, skip_listed);
        // A list is not a run of progress windows.
        for id in done.new {
            self.session.own(id);
        }
        let (added, unread) = (done.added, done.unread);
        self.say(if unread > 0 {
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
        self.read();
        cx.notify();
    }

    /// Export: the list written where the system's save panel says, off
    /// the window's thread (a long list is a long file), and said once it
    /// is.
    pub(crate) fn export(&mut self, cx: &mut Context<Self>) {
        crate::services::ask_then(
            crate::services::Asked::file(true, "downloads.dmlist"),
            cx,
            |view: &mut Self, path, cx| {
                view.later(
                    move |backend| {
                        let written = std::fs::write(&path, backend.export());
                        (path, written)
                    },
                    |view, (path, written), _| match written {
                        Ok(()) => view.say(tf("Exported to {}.", &[&path.display()])),
                        Err(e) => view.say(tf(
                            "{} could not be written: {}",
                            &[&path.display(), &dm_desktop_base::format::file_error(&e)],
                        )),
                    },
                    cx,
                );
            },
        );
    }
}
