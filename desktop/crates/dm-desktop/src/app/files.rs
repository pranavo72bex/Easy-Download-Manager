// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

impl DmApp {
    /// Opens `path` with an application the person picks: from the row
    /// menu and from Download complete.
    pub fn open_with_chosen(&mut self, path: &str) {
        // Nothing to pick an application with: said, rather than a button
        // that did nothing.
        if !self.backend.platform.can_choose_file() {
            self.said = crate::i18n::Said::new(
                "This desktop has no app picker, so Open uses the file's usual app.",
                &[],
            );
        } else if let Some(app) = self.backend.platform.choose_app() {
            if let Err(e) = self.backend.platform.open_with(path, &app) {
                self.said = e.into();
            }
        }
    }

    /// Opens `path` with the system's opener, off the window's thread.
    pub fn open_file(&self, path: &str) {
        let said = Arc::clone(&self.opener_said);
        let report: crate::power::Report = Box::new(move |why| *dm_core::lock(&said) = Some(why));
        self.backend.platform.open_later(path, report);
    }

    /// Shows `path` in the file manager, off the window's thread.
    /// Extracts a finished ZIP into a new folder beside it, off the
    /// window's thread; the answer arrives in the status line the way an
    /// opener's does. Nothing in it is opened or run.
    pub fn extract_zip(&mut self, path: &str, url: &str, into: Option<std::path::PathBuf>) {
        let said = Arc::clone(&self.opener_said);
        self.said = crate::i18n::t("Extracting…").into();
        // Done off the window's thread by the application; only the words
        // are the window's.
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
            self.said = format!("Not extracted: {e}").into();
        }
    }

    /// Shows `path` in the system's file manager.
    pub fn reveal_file(&self, path: &str) {
        let said = Arc::clone(&self.opener_said);
        let report: crate::power::Report = Box::new(move |why| *dm_core::lock(&said) = Some(why));
        self.backend.platform.reveal_later(path, report);
    }

    /// Whether the schedule is closed with downloads waiting on it, and when
    /// it next opens.
    pub fn schedule_holds(&self) -> Option<Option<String>> {
        let schedule = &self.applied.schedule;
        dm_shell::app::schedule::holds_waiting(schedule, &self.rows).then(|| opens_next(schedule))
    }
}
