// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

impl DmApp {
    /// Imports the list in the file at `path`. Addresses already in the
    /// list are asked about first, as Add URL asks.
    pub fn import_from(&mut self, path: &std::path::Path) -> Result<(), String> {
        // A .torrent is a download, not a list of them: read as text it was
        // "not valid UTF-8".
        if let Some(local) = crate::dialogs::local_torrent(&path.to_string_lossy()) {
            return match self.add(&[local], Default::default()) {
                Ok(_) => Ok(()),
                Err(e) => {
                    self.said = e.clone().into();
                    Err(e)
                }
            };
        }
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                self.said = crate::i18n::Said::new(
                    "{} could not be read: {}",
                    &[&path.display(), &format::file_error(&e)],
                );
                return Err(self.said.shown());
            }
        };
        let (already, total) = dm_shell::app::import::already_listed(&self.rows, &text);
        if already > 0 {
            self.dialogs.import_ask = Some(crate::dialogs::ImportAsk {
                text,
                already,
                total,
            });
            return Ok(());
        }
        self.import_text(&text, false);
        Ok(())
    }

    /// Adds the downloads a list holds; `skip_listed`, not those whose
    /// address is already a row.
    pub fn import_text(&mut self, text: &str, skip_listed: bool) {
        // What is added, skipped and unread is the application's.
        let done = dm_shell::app::import::import(&self.backend, &self.rows, text, skip_listed);
        // A list is not a run of progress windows.
        for id in done.new {
            self.session.own(id);
        }
        let (added, unread) = (done.added, done.unread);
        self.said = if unread > 0 {
            crate::i18n::Said::naming(
                "Imported {}. Skipped {} lines with no link in them (a link starts with http://, https://, ftp:// or magnet:).",
                &[&added, &unread],
            )
        } else {
            crate::i18n::Said::counting(
                "Imported {} download.||Imported {} downloads.",
                added as u64,
                &[&added],
            )
        };
        self.refresh();
    }

    /// Writes the list to `path`, and says where or why not.
    pub fn export_to(&mut self, path: &std::path::Path) -> Result<(), String> {
        match std::fs::write(path, self.backend.export()) {
            Ok(()) => {
                self.said = crate::i18n::Said::new("Exported to {}.", &[&path.display()]);
                Ok(())
            }
            Err(e) => {
                self.said = crate::i18n::Said::new(
                    "{} could not be written: {}",
                    &[&path.display(), &format::file_error(&e)],
                );
                Err(self.said.shown())
            }
        }
    }
}
