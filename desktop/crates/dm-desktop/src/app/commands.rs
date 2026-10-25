use super::*;

impl DmApp {
    pub fn enabled(&self, command: Command) -> bool {
        // The selected rows as they stand, neither copied nor sorted: asked
        // for every button and menu item each frame, `targets` copied and
        // sorted the whole list each time, which a thousand rows made slow.
        // Order does not matter to whether a command applies.
        let targets: Vec<&Entry> = self
            .rows
            .iter()
            .filter(|r| self.selected.contains(&r.id) && self.shows(r))
            .collect();
        let any = |f: fn(&Entry) -> bool| targets.iter().any(|r| f(r));
        match command {
            Command::DownloadNow => any(|r| matches!(r.state, State::Paused | State::Failed(_))),
            Command::StopDownload => any(|r| matches!(r.state, State::Running | State::Queued)),
            // Enabled only for downloads neither running nor in line —
            // a running one is stopped first, so nothing is removed from
            // under its own writer by a slip of the Delete key.
            Command::Remove => {
                !targets.is_empty()
                    && targets
                        .iter()
                        .all(|r| !matches!(r.state, State::Running | State::Queued))
            }
            Command::MoveToQueue | Command::Properties => !targets.is_empty(),
            // Only where there is something to fetch again: a row that has
            // fetched nothing yet would only start.
            // A failure with nothing fetched is Retry's.
            Command::Redownload => any(|r| {
                matches!(r.state, State::Completed | State::Cancelled) || r.progress.downloaded > 0
            }),
            Command::CopyLink => !targets.is_empty(),
            Command::UpdateLink => {
                targets.len() == 1
                    && (!matches!(targets[0].state, State::Running | State::Completed)
                        || retrying(targets[0].id).is_some())
            }
            Command::RemoveFromQueue => any(|r| !r.queue.is_empty()),
            // Not where it would do nothing: at the top or the bottom of
            // the order.
            Command::MoveUp => {
                targets.len() == 1 && self.rows.first().is_some_and(|r| r.id != targets[0].id)
            }
            Command::MoveDown => {
                targets.len() == 1 && self.rows.last().is_some_and(|r| r.id != targets[0].id)
            }
            Command::ShowProgress => any(|r| r.state != State::Completed),
            // Open folder stays: it says the file is gone, which is an answer.
            Command::OpenFolder => {
                targets.len() == 1
                    && targets[0].state == State::Completed
                    && !targets[0].path.is_empty()
            }
            // Something a player can show, with some of it fetched.
            Command::Preview => {
                targets.len() == 1
                    && targets[0].state != State::Completed
                    && targets[0].progress.downloaded > 0
                    && matches!(
                        kind_of(targets[0]),
                        Some(Category::Videos | Category::Audio)
                    )
            }
            // A finished ZIP that is still on disk.
            Command::ExtractHere | Command::ExtractTo => {
                targets.len() == 1
                    && targets[0].state == State::Completed
                    && dm_core::extract::is_zip(&targets[0].path)
                    && !crate::table::gone(targets[0])
            }
            // A finished file that is still on disk.
            Command::ShareOnNetwork => {
                targets.len() == 1
                    && targets[0].state == State::Completed
                    && !targets[0].path.is_empty()
                    && !crate::table::gone(targets[0])
            }
            // While a file is being shared on the network.
            Command::StopSharing => self.share.is_some(),
            Command::OpenFile | Command::OpenWith => {
                targets.len() == 1
                    && targets[0].state == State::Completed
                    && !targets[0].path.is_empty()
                    && !crate::table::gone(targets[0])
            }
            // One to move and rename, or several to move together.
            Command::MoveRename => {
                !targets.is_empty()
                    && targets.iter().all(|r| {
                        r.state == State::Completed && !r.path.is_empty() && !crate::table::gone(r)
                    })
            }
            Command::ResumeAll => self.rows.iter().any(|r| r.state == State::Paused),
            Command::StopAll => self
                .rows
                .iter()
                .any(|r| matches!(r.state, State::Running | State::Queued)),
            Command::DeleteCompleted => self.rows.iter().any(|r| r.state.clearable()),
            Command::FindNext => !self.dialogs.find_text.is_empty(),
            _ => true,
        }
    }

    /// Runs one command, as a menu item, button, key or the control
    /// protocol asked for it.
    pub fn run(&mut self, command: Command) {
        if self.by_script()
            && matches!(
                command,
                Command::OpenFile | Command::OpenWith | Command::Preview
            )
        {
            self.said = "Opening a file is done by a person, not by a script"
                .to_string()
                .into();
            return;
        }
        if self.by_script() && command == Command::ShareOnNetwork {
            self.said = "Sharing a file is done by a person, not by a script"
                .to_string()
                .into();
            return;
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
            Command::AddUrl => self.dialogs.open_add_url(self.backend.platform.clipboard()),
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
                let held: Vec<Id> = targets
                    .iter()
                    .filter(|r| r.state == State::Paused && r.paused_by_schedule)
                    .map(|r| r.id)
                    .collect();
                if !held.is_empty() {
                    if self.headless {
                        self.said =
                            crate::i18n::Said::new("Waiting for the schedule or speed limit.", &[]);
                    } else {
                        self.dialogs.confirm_outside = Some(held);
                    }
                }
                for row in &targets {
                    match row.state {
                        // Held: asking would only requeue it to be held again.
                        State::Paused if row.paused_by_schedule => {}
                        State::Paused => self.backend.resume(row.id),
                        State::Failed(_) => self.backend.retry(row.id),
                        _ => {}
                    }
                    // Not one its person closed: fifteen Resumes opened it
                    // fifteen times over the list. Show progress
                    // still opens it.
                    if self.applied.show_progress && !self.dialogs.progress_closed.contains(&row.id)
                    {
                        self.dialogs.progress.insert(row.id);
                    }
                }
            }
            Command::StopDownload => {
                for row in &targets {
                    self.backend.pause(row.id);
                }
            }
            Command::Remove => {
                // Asked first, whichever way it was asked for: an
                // unfinished download's fetched bytes go with it, and a
                // finished one leaves the list.
                // Headless, with nobody to ask, finished rows go as before.
                let ids: Vec<Id> = targets.iter().map(|r| r.id).collect();
                if self.headless && targets.iter().all(|r| r.state == State::Completed) {
                    self.remove_now(&ids);
                } else {
                    self.dialogs.confirm_remove = Some(ids);
                }
            }
            Command::Redownload => {
                for row in &targets {
                    self.backend.redownload(row.id);
                }
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
            Command::RemoveFromQueue => {
                for row in &targets {
                    self.backend.set_queue(row.id, "");
                }
            }
            Command::MoveUp => self.backend.shift(targets[0].id, -1),
            Command::MoveDown => self.backend.shift(targets[0].id, 1),
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
            Command::ShareOnNetwork => self.share_on_network(&targets[0].path),
            Command::StopSharing => self.stop_sharing(),
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
                self.selected = self.visible().iter().map(|r| r.id).collect();
            }

            Command::ResumeAll => {
                self.backend.resume_all();
            }
            Command::StopAll => {
                self.backend.pause_all();
            }
            Command::DeleteCompleted => {
                let n = self.backend.clear_finished();
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
            Command::Scheduler => self.dialogs.open_scheduler(&self.applied),
            Command::StartQueue => {
                let queue = self.current_queue();
                let n = self.backend.start_queue(&queue);
                // "Started 0 downloads" said nothing useful.
                self.said = if n == 0 {
                    crate::i18n::Said::naming("Nothing in {} was waiting to start.", &[&queue])
                } else if self.applied.schedule.enabled && !self.applied.schedule.open() {
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
                // Asked in the background: a server at a time, each up to
                // its timeouts, which held the window for minutes when it
                // was done here. The counts are said when it is done.
                if queue == dm_core::RECHECK_QUEUE {
                    let (q, done) = (self.backend.queue.clone(), Arc::clone(&self.rechecked));
                    self.backend.runtime.spawn(async move {
                        let (checked, changed) = q.synchronize(dm_core::RECHECK_QUEUE).await;
                        if !changed.is_empty() {
                            dm_shell::shell::wake(&q);
                        }
                        *dm_core::lock(&done) = Some((checked, changed.len()));
                    });
                }
            }
            Command::StopQueue => {
                let queue = self.current_queue();
                let n = self.backend.stop_queue(&queue);
                self.said = crate::i18n::Said::counting_naming(
                    "Stopped {} download in {}.||Stopped {} downloads in {}.",
                    n as u64,
                    &[&n, &queue],
                );
            }
            Command::SpeedLimiterToggle => {
                let mut wanted = self.applied.clone();
                if wanted.speed_limit > 0 {
                    self.last_limit = wanted.speed_limit;
                    wanted.last_speed_limit = wanted.speed_limit;
                    wanted.speed_limit = 0;
                } else {
                    wanted.speed_limit = self.last_limit.max(1);
                }
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
                let mut wanted = self.applied.clone();
                if wanted.speed_limit > 0 {
                    self.last_limit = wanted.speed_limit;
                    wanted.last_speed_limit = wanted.speed_limit;
                }
                wanted.speed_limit = kb;
                match self.apply_settings(wanted) {
                    Ok(()) if kb > 0 => {
                        self.said = crate::i18n::Said::new("Speed limiter on: {} KB/s.", &[&kb])
                    }
                    Ok(()) => self.said = crate::i18n::Said::new("Speed limiter off.", &[]),
                    Err(e) => self.said = e.into(),
                }
            }
            Command::WhenDoneNothing => self.when_done = crate::power::WhenDone::Nothing,
            Command::WhenDoneQuit => self.when_done = crate::power::WhenDone::Quit,
            Command::WhenDoneSleep => self.when_done = crate::power::WhenDone::Sleep,
            Command::WhenDoneShutDown => self.when_done = crate::power::WhenDone::ShutDown,
            Command::WhenDoneHibernate => self.when_done = crate::power::WhenDone::Hibernate,
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
