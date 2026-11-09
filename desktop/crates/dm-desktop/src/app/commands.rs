// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use dm_shell::app::commands::{limited, recheck, verb, when_done, Ran};

/// What the window knows of a row that decides whether a command applies:
/// read where it is kept for drawing.
struct WindowLooks;

impl dm_shell::app::commands::Looks for WindowLooks {
    fn retrying(&self, id: Id) -> bool {
        retrying(id).is_some()
    }

    fn gone(&self, row: &Entry) -> bool {
        crate::table::gone(row)
    }

    fn plays(&self, row: &Entry) -> bool {
        matches!(kind_of(row), Some(Category::Videos | Category::Audio))
    }
}

impl DmApp {
    /// Whether `command` applies to the rows selected now.
    #[must_use]
    pub fn enabled(&self, command: Command) -> bool {
        // The selected rows as they stand, neither copied nor sorted: asked
        // for every button and menu item each frame, `targets` copied and
        // sorted the whole list each time, which a thousand rows made slow.
        // Order does not matter to whether a command applies.
        let targets: Vec<&Entry> = self
            .rows
            .iter()
            .map(|r| &**r)
            .filter(|r| self.selected.contains(&r.id) && self.shows(r))
            .collect();
        dm_shell::app::commands::applies(
            command,
            &targets,
            &self.rows,
            !self.dialogs.find_text.is_empty(),
            &WindowLooks,
        )
    }

    /// Whether the schedule that governs `queue` (its own, or the one every
    /// queue shares) lets it run now.
    fn schedule_lets(&self, queue: &str) -> bool {
        let schedule = self.applied.schedule_for(queue);
        !schedule.enabled || schedule.open()
    }

    /// Runs one command, as a menu item, button, key or the control
    /// protocol asked for it.
    pub fn run(&mut self, command: Command) {
        if matches!(
            command,
            Command::OpenFile | Command::OpenWith | Command::Preview
        ) {
            if let Some(why) = self.run_refused() {
                self.said = why.to_string().into();
                return;
            }
        }
        if !self.enabled(command) {
            return;
        }
        // A question first: only Exit, which asks one of its own, gets past.
        if self.dialogs.asking() && command != Command::Quit {
            return;
        }
        let targets = self.targets();
        match command {
            Command::AddUrl => self
                .dialogs
                .open_add_url(self.backend.platform.clipboard().as_deref()),
            Command::AddSeries => self.dialogs.open_series(),
            Command::AddFromClipboard => {
                let text = self.backend.platform.clipboard().unwrap_or_default();
                let found = links_in(&text);
                if found.is_empty() {
                    // In the language in force (English always).
                    self.said = crate::i18n::Said::new("There are no links on the clipboard.", &[]);
                } else {
                    // One already in the list is offered unticked, and said
                    // so: it was fetched again as `e (1).zip`.
                    let listed = crate::dialogs::listed_addresses(self);
                    self.dialogs.clip_links = Some(
                        found
                            .into_iter()
                            .map(|l| {
                                let new = !listed.contains(&crate::dialogs::bare(&l));
                                (l, new)
                            })
                            .collect(),
                    );
                }
            }
            Command::Import => {
                if !self.backend.platform.can_choose_file() {
                    // No chooser to ask with: the path is typed.
                    self.dialogs.focus_on("List file");
                    self.dialogs.list_file = Some(crate::dialogs::ListFile::default());
                } else if let Some(path) = self.backend.platform.choose_file(false, "") {
                    let _ = self.import_from(&path);
                }
            }
            Command::Export => {
                if !self.backend.platform.can_choose_file() {
                    self.dialogs.focus_on("List file");
                    self.dialogs.list_file = Some(crate::dialogs::ListFile {
                        save: true,
                        path: crate::dialogs::default_list_path(&self.applied)
                            .display()
                            .to_string(),
                        says: String::new(),
                    });
                } else if let Some(path) =
                    self.backend.platform.choose_file(true, "downloads.dmlist")
                {
                    let _ = self.export_to(&path);
                }
            }
            Command::SiteFiles => self.dialogs.open_site_files(),
            Command::Quit => self.ask_quit(),

            Command::DownloadNow => {
                // A row the schedule or the download limit holds is asked
                // about: start it now, outside the schedule, or let it wait.
                // With nobody to ask (headless), it is said instead.
                // Each row goes on (the application's `verb`) but those the
                // schedule or the download limit holds.
                let held = match verb(command, &self.backend, &targets, "") {
                    Some(Ran::WentOn { held }) => held,
                    _ => Vec::new(),
                };
                if !held.is_empty() {
                    if self.headless {
                        self.said =
                            crate::i18n::Said::new("Waiting for the schedule or speed limit.", &[]);
                    } else {
                        self.dialogs.confirm_outside = Some(held);
                    }
                }
                for row in &targets {
                    // Not one its person closed: fifteen Resumes opened it
                    // fifteen times over the list. Show progress
                    // still opens it.
                    if self.applied.show_progress && !self.dialogs.progress_closed.contains(&row.id)
                    {
                        self.dialogs.progress.insert(row.id);
                    }
                }
            }
            // The application's alone, with nothing to say.
            Command::StopDownload
            | Command::Redownload
            | Command::RemoveFromQueue
            | Command::MoveUp
            | Command::MoveDown
            | Command::ResumeAll
            | Command::StopAll => {
                verb(command, &self.backend, &targets, "");
            }
            Command::Remove => {
                // Asked first, whichever way it was asked for: an
                // unfinished download's fetched bytes go with it, and a
                // finished one leaves the list.
                // Headless, with nobody to ask, finished rows go as before.
                // A finished download leaves the list without a question, as
                // nothing is lost (its file stays) and a slip can be undone
                // for a few seconds; Delete file is the one that asks.
                let ids: Vec<Id> = targets.iter().map(|r| r.id).collect();
                if targets.iter().all(|r| r.state == State::Completed) {
                    self.remove_finished_with_undo(&ids);
                } else {
                    self.dialogs.confirm_remove = Some(ids);
                }
            }
            Command::DeleteFile => {
                let ids: Vec<Id> = targets.iter().map(|r| r.id).collect();
                self.dialogs.remove_files = true;
                self.dialogs.deleting_files = true;
                self.dialogs.confirm_remove = Some(ids);
            }
            Command::CopyLink => {
                // Each address on a line of its own, without a sign-in
                // that may be in it: what is pasted elsewhere is the link.
                let links: Vec<String> = targets
                    .iter()
                    .map(|r| format::copyable_url(&r.url))
                    .collect();
                self.copied = Some(links.join("\n"));
                self.said = crate::i18n::Said::counting(
                    "Copied {} link.||Copied {} links.",
                    links.len() as u64,
                    &[&links.len()],
                );
            }
            Command::CopyDetails => self.copy_details_of(&targets),
            Command::UpdateLink => {
                self.dialogs.focus_on("Update download link");
                self.dialogs.update_link = Some((targets[0].id, String::new(), String::new()))
            }
            Command::MoveToQueue => {
                self.dialogs.focus_on("Move to queue");
                self.dialogs.move_to_queue = Some((
                    targets.iter().map(|r| r.id).collect(),
                    // As the dialog shows it; taken back to dm's own on OK.
                    crate::i18n::name(MAIN_QUEUE),
                    String::new(),
                ))
            }
            Command::Properties => {
                for row in &targets {
                    self.dialogs.open_properties(row);
                }
            }
            Command::ShowProgress => {
                for row in targets.iter().filter(|r| r.state != State::Completed) {
                    self.dialogs.progress.insert(row.id);
                }
            }
            Command::OpenFile => {
                self.open_file(&targets[0].path);
            }
            Command::Preview => match self.backend.preview(targets[0].id) {
                Ok(path) => self.open_file(&path.to_string_lossy()),
                // Nothing on disk yet is the usual answer, and one to word.
                Err(e) if e.starts_with("nothing") => {
                    self.said = crate::i18n::t("Nothing of it is on disk to preview yet.").into()
                }
                Err(e) => self.said = e.into(),
            },
            Command::OpenWith => {
                let path = targets[0].path.clone();
                self.open_with_chosen(&path);
            }
            Command::OpenFolder => {
                self.reveal_file(&targets[0].path);
            }
            Command::ExtractHere => {
                self.extract_zip(&targets[0].path, &targets[0].url, None);
            }
            Command::ExtractTo => {
                if !self.backend.platform.can_choose_folder() {
                    self.said = crate::i18n::t("No folder picker here. Use Extract here.").into();
                } else if let Some(folder) = self.backend.platform.choose_folder() {
                    self.extract_zip(&targets[0].path, &targets[0].url, Some(folder));
                }
            }
            Command::MoveRename => {
                self.dialogs.focus_on("Move/Rename");
                self.dialogs.rename = Some(crate::dialogs::MoveRename {
                    ids: targets.iter().map(|r| r.id).collect(),
                    name: targets[0].filename.clone(),
                    folder: std::path::Path::new(&targets[0].path)
                        .parent()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    pattern: String::new(),
                })
            }
            Command::SelectAll => {
                self.selected = self.visible_shared().iter().map(|r| r.id).collect();
            }

            Command::DeleteCompleted => {
                let n = match verb(command, &self.backend, &targets, "") {
                    Some(Ran::Cleared(n)) => n,
                    _ => 0,
                };
                self.said = crate::i18n::Said::counting(
                    "Deleted {} completed or cancelled download from the list.||Deleted {} completed or cancelled downloads from the list.",
                    n as u64, &[&n],
                );
            }
            Command::Find => {
                self.dialogs.find_open = true;
                self.dialogs.focus_on("Find");
            }
            Command::FindNext => self.find_next(),
            Command::FindPrevious => self.find_previous(),
            Command::Scheduler => {
                let queue = self.current_queue();
                self.dialogs.open_scheduler(&self.applied, &queue);
            }
            Command::StartQueue => {
                let queue = self.chosen_queue();
                let n = match verb(command, &self.backend, &targets, &queue) {
                    Some(Ran::QueueStarted(n)) => n,
                    _ => 0,
                };
                // Its own sound, only where the person chose a file.
                if n > 0 {
                    if let Some(path) = self.sound_file("queue_started") {
                        self.backend.platform.play_file(&path);
                    }
                }
                // "Started 0 downloads" said nothing useful.
                self.said = if n == 0 {
                    crate::i18n::Said::naming("Nothing in {} was waiting to start.", &[&queue])
                } else if !self.schedule_lets(&queue) {
                    // "Started" while the schedule held them, which nothing
                    // was.
                    crate::i18n::Said::naming(
                        "{} is waiting for the schedule; it starts by itself when the schedule opens.",
                        &[&queue],
                    )
                } else {
                    crate::i18n::Said::counting_naming(
                        "Started {} download in {}.||Started {} downloads in {}.",
                        n as u64,
                        &[&n, &queue],
                    )
                };
                // Asked in the background (the application's `recheck`); the
                // counts are said when it is done.
                if queue == dm_core::RECHECK_QUEUE {
                    recheck(&self.backend, Arc::clone(&self.rechecked));
                }
            }
            Command::StopQueue => {
                let queue = self.chosen_queue();
                let n = match verb(command, &self.backend, &targets, &queue) {
                    Some(Ran::QueueStopped(n)) => n,
                    _ => 0,
                };
                self.said = crate::i18n::Said::counting_naming(
                    "Stopped {} download in {}.||Stopped {} downloads in {}.",
                    n as u64,
                    &[&n, &queue],
                );
            }
            Command::SpeedLimiterToggle => {
                let wanted = limited(command, &self.applied, &mut self.last_limit)
                    .unwrap_or_else(|| self.applied.clone());
                if let Err(e) = self.apply_settings(wanted) {
                    self.said = e.into();
                } else {
                    self.said = if self.applied.speed_limit > 0 {
                        crate::i18n::Said::new(
                            "Speed limiter on: {} KB/s.",
                            &[&self.applied.speed_limit],
                        )
                    } else {
                        crate::i18n::Said::new("Speed limiter off.", &[])
                    };
                }
            }
            Command::SpeedLimiterSettings => self.dialogs.open_limiter(&self.applied),
            Command::SpeedLow
            | Command::SpeedMedium
            | Command::SpeedHigh
            | Command::SpeedUnlimited => {
                let kb = command.speed_preset().unwrap_or(0);
                let wanted = limited(command, &self.applied, &mut self.last_limit)
                    .unwrap_or_else(|| self.applied.clone());
                match self.apply_settings(wanted) {
                    Ok(()) if kb > 0 => {
                        self.said = crate::i18n::Said::new("Speed limiter on: {} KB/s.", &[&kb])
                    }
                    Ok(()) => self.said = crate::i18n::Said::new("Speed limiter off.", &[]),
                    Err(e) => self.said = e.into(),
                }
            }
            Command::WhenDoneNothing
            | Command::WhenDoneQuit
            | Command::WhenDoneSleep
            | Command::WhenDoneShutDown
            | Command::WhenDoneHibernate => {
                if let Some(chosen) = when_done(command) {
                    self.session.when_done = chosen;
                }
            }
            Command::Options => self.dialogs.open_options(&self.applied),

            Command::ToggleCategories => self.show_categories = !self.show_categories,
            Command::ToggleToolbar => self.show_toolbar = !self.show_toolbar,
            Command::ToggleDropTarget => {
                let mut wanted = self.applied.clone();
                wanted.drop_target = !wanted.drop_target;
                if let Err(e) = self.apply_settings(wanted) {
                    self.said = e.into();
                }
            }
            Command::SortByName => self.sort_by(Column::Name),
            Command::SortBySize => self.sort_by(Column::Size),
            Command::SortByStatus => self.sort_by(Column::Status),
            Command::SortByTimeLeft => self.sort_by(Column::TimeLeft),
            Command::SortByRate => self.sort_by(Column::Rate),
            Command::SortByDate => self.sort_by(Column::LastTry),
            Command::SortByDescription => self.sort_by(Column::Description),

            Command::About => self.dialogs.about_open = true,
            Command::Help => {
                if let Err(e) = self.backend.platform.open(crate::update::HELP) {
                    self.said = e.into();
                }
            }
            Command::CheckForUpdates => {
                self.dialogs.about_open = true;
                self.dialogs.check_update = true;
            }
        }
        self.refresh();
    }
}

impl DmApp {
    /// "Copy details": the support block of each failed row among `rows` (`crate::details`), onto
    /// the clipboard: what a person can paste to somebody who is helping them, with nothing secret
    /// in it.
    pub fn copy_details_of(&mut self, rows: &[Entry]) {
        let Some((text, count)) =
            crate::details::blocks_for(rows, self.applied.connections, &self.applied.download_dir)
        else {
            return;
        };
        self.copied = Some(text);
        self.said = crate::i18n::Said::counting(
            "Copied the details of {} download.||Copied the details of {} downloads.",
            count as u64,
            &[&count],
        );
    }
}
