// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

impl DmApp {
    /// A handed-over download from `site` was cancelled in Download
    /// details. Twice running, and the person is asked whether that site's
    /// downloads should stay with the browser; a site already left there,
    /// or one asked about, is not asked about again.
    pub fn hand_off_cancelled(&mut self, site: &str) {
        if self.applied.catch.skip_hosts.iter().any(|h| h == site) {
            return;
        }
        let count = self
            .cancelled_hand_offs
            .entry(site.to_string())
            .or_insert(0);
        *count += 1;
        if *count >= 2 {
            *count = 0;
            self.dialogs.skip_offer = Some(site.to_string());
        }
    }

    /// Reads the list back from the queue and does what follows from it:
    /// announcing what finished, collecting problems.
    pub fn refresh(&mut self) {
        // Before the list is read, so a change made while it is read is
        // still owed a read afterwards.
        QUEUE_CHANGED.store(false, std::sync::atomic::Ordering::Release);
        self.take_when_done();
        self.take_new_download();
        self.rows = self.backend.list_shared();
        self.rows_gen += 1;
        self.place_rows();
        set_retrying(self.backend.retrying());
        set_swarms(self.backend.swarms());
        crate::table::set_superseded(&self.rows);
        set_limit_holds(self.backend.queue.allowance_spent().is_some());
        self.saved_sizes();
        self.watch_asks();
        self.apply_pending_renames();
        self.refreshed = Some(Instant::now());
        let ids: HashSet<Id> = self.rows.iter().map(|r| r.id).collect();
        self.selected.retain(|id| ids.contains(id));
        self.forget_said_about();

        let mut finished = Vec::new();
        let mut toasts: Vec<(String, String)> = Vec::new();
        let mut failed = Vec::new();
        let mut expired = Vec::new();
        for row in &self.rows {
            // Once per failure: a row retried and back in line is told
            // again when it fails again.
            if !matches!(row.state, State::Failed(_)) {
                self.failed_told.remove(&row.id);
            }
            if matches!(row.state, State::Failed(_))
                && self.failed_told.insert(row.id)
                && self.notify_ready
            {
                failed.push(row.id);
                if link_expired(row) {
                    expired.push(row.id);
                }
                // Told as a finish is, with why in plain words: a failure
                // was only a sound, if that was on.
                self.backend.platform.notify(
                    t("Download failed"),
                    &format!("{} — {}", name_of(row), status_of(row)),
                );
            }
            if row.state == State::Completed && self.notified.insert(row.id) && self.notify_ready {
                toasts.push((name_of(row), row.path.clone()));
                finished.push(row.id);
                // The command a person set to run on each finished file.
                let command = self.applied.on_complete.trim();
                if !command.is_empty() && !row.path.is_empty() {
                    if let Err(why) =
                        dm_shell::shell::run_after(command, std::path::Path::new(&row.path))
                    {
                        self.problems.push(why);
                    }
                }
            }
        }
        // One toast for what finished in this pass, not one per file: a batch
        // is one sentence, a single file keeps its Open and Open folder.
        match toasts.as_slice() {
            [] => {}
            [(name, path)] => self
                .backend
                .platform
                .notify_file(t("Download finished"), name, path),
            many => self.backend.platform.notify(
                t("Download finished"),
                &tf("{} downloads finished.", &[&many.len()]),
            ),
        }
        if self.applied.sound_on_complete && !finished.is_empty() {
            match self.sound_file("complete") {
                Some(path) => self.backend.platform.play_file(&path),
                None => self.backend.platform.chime(),
            }
        }
        if self.applied.sound_on_failure && !failed.is_empty() {
            match self.sound_file("failed") {
                Some(path) => self.backend.platform.play_file(&path),
                None => self.backend.platform.alert(),
            }
        }
        self.schedule_hooks();
        self.when_queue_dry();
        for id in finished {
            /* Over, so its progress window is: left open it said "Complete"
             * with no button to press, and one for each finished download
             * stacked behind the next. With the complete window on, that
             * comes in its place. */
            self.dialogs.progress.remove(&id);
            if self.applied.show_complete {
                self.dialogs.complete.insert(id);
            }
        }
        // A link that stopped working part-way, as signed links run out:
        // asked for a fresh one at once, which goes on from the parts
        // already here, rather than left for a person to know of Update
        // download link. Not over another dialog; the row's own failure
        // says Retry, and the row menu has it, either way.
        if let Some(id) = expired.first() {
            if self.applied.offer_new_link
                && !self.headless
                && !self.dialogs.in_front()
                && self.dialogs.update_link.is_none()
            {
                self.dialogs.focus_on("Update download link");
                self.dialogs.update_link = Some((*id, String::new(), String::new()));
            }
        }
        // A download that started without the window starting it — handed
        // over by a browser, or by a second launch — opens its progress
        // window as one started here does, where that is asked for. Not
        // the rows there when the window opened, and not one added for
        // later, which has not started.
        if self.applied.show_progress && self.notify_ready && !self.headless {
            let started: Vec<Id> = self
                .rows
                .iter()
                .filter(|row| {
                    !self.last_state.contains_key(&row.id)
                        && !self.own.contains(&row.id)
                        && matches!(row.state, State::Running | State::Queued)
                })
                .map(|row| row.id)
                .collect();
            // One window for one download: a batch arriving at once opened a
            // stack of them over the list (a design review counted ten).
            if let [only] = started.as_slice() {
                self.dialogs.progress.insert(*only);
            }
        }
        // A browser's hand-off of an address already listed: added by the
        // inbox before the window hears of it, and the browser has let its
        // own download go, so there is nothing left to ask. Said, as Add
        // URL would have asked.
        if self.notify_ready {
            let fresh = |row: &&Arc<Entry>| {
                !self.last_state.contains_key(&row.id) && !self.own.contains(&row.id)
            };
            let again = self.rows.iter().filter(fresh).find(|row| {
                let address = crate::dialogs::bare(&row.url);
                self.rows.iter().any(|other| {
                    other.id != row.id
                        && other.state != State::Cancelled
                        && self.last_state.contains_key(&other.id)
                        && crate::dialogs::bare(&other.url) == address
                })
            });
            if let Some(row) = again {
                self.said = crate::i18n::Said::new(
                    "Added {} again: that address was already in the list.",
                    &[&name_of(row)],
                );
            }
        }
        self.own.retain(|id| ids.contains(id));
        // A size told by a dialog stands only until the row has its own.
        let rows = &self.rows;
        self.sizes_told.retain(|id, _| {
            rows.iter()
                .any(|r| r.id == *id && r.progress.total.is_none() && r.progress.downloaded == 0)
        });
        self.last_state = self.rows.iter().map(|r| (r.id, r.state.clone())).collect();
        // After the first pass every completion already on disk is seeded.
        self.notify_ready = true;
        self.problems.extend(
            self.backend
                .problems()
                .into_iter()
                .map(|p| worded_problem(&p)),
        );
    }

    /// A finished row's size is its file's: read once from the disk for
    /// each path, and put in place of the bytes the engine counted, which
    /// for a stream remuxed into an MP4 were the joined stream's.
    pub(super) fn saved_sizes(&mut self) {
        let saved = &mut self.saved;
        saved.retain(|id, _| {
            self.rows
                .iter()
                .any(|r| r.id == *id && r.state == State::Completed)
        });
        for row in &mut self.rows {
            if row.state != State::Completed || row.path.is_empty() {
                continue;
            }
            let known = saved.get(&row.id).filter(|(path, _)| *path == row.path);
            let size = match known {
                Some((_, size)) => *size,
                None => match std::fs::metadata(&row.path) {
                    Ok(meta) if meta.is_file() => {
                        saved.insert(row.id, (row.path.clone(), meta.len()));
                        meta.len()
                    }
                    _ => continue,
                },
            };
            // Copied only where the figures differ: an unchanged row stays
            // the queue's own, which is how the next read knows it.
            if row.progress.total != Some(size) || row.progress.downloaded != size {
                let row = Arc::make_mut(row);
                row.progress.total = Some(size);
                row.progress.downloaded = size;
            }
        }
    }

    /// "When done": once everything that was downloading has settled,
    /// quit, or start the countdown to sleep or shut down. Once only: the
    /// choice goes back to doing nothing.
    ///
    /// Settled means nothing is left that dm will finish by itself: nothing
    /// Running or Queued, and nothing the schedule or the download limit is
    /// holding (Paused with `paused_by_schedule`) — those start again when
    /// the window reopens or the allowance rolls over, so quitting or
    /// sleeping under them leaves a download half done that nothing resumes.
    ///
    /// A row a person stopped is theirs: it is left alone, not waited for,
    /// and does not keep the choice from firing once the rest has finished.
    /// A Failed row is settled too — nothing retries it without a person,
    /// and its parts stay for Retry whatever the computer does next. But a
    /// stretch that ended with nothing finished or failed (a person pressed
    /// Stop all) is not "done": the choice stays armed for the next one.
    /// The sound file chosen for `event`, if one was.
    pub(super) fn sound_file(&self, event: &str) -> Option<String> {
        self.applied
            .sounds
            .get(event)
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
    }

    pub(super) fn when_queue_dry(&mut self) {
        if !self.dry.look(&self.rows, &self.last_state) {
            return;
        }
        // A queue that has run dry has finished: its own sound, only where the person chose a file.
        if let Some(path) = self.sound_file("queue_done") {
            self.backend.platform.play_file(&path);
        }
        use crate::power::WhenDone;
        match std::mem::take(&mut self.when_done) {
            WhenDone::Nothing => {}
            // With nobody to see a countdown, at once.
            WhenDone::Quit if self.headless => self.quit_asked = true,
            // The others' countdown for Exit too: it quit the moment the
            // last download ended, with no word.
            action => {
                let at = Instant::now() + Duration::from_secs(crate::power::COUNTDOWN_SECS);
                self.countdown = Some((action, at));
            }
        }
    }

    /// A video the browser's panel handed over: "Download details",
    /// one at a time. With no window to ask in, it is simply added.
    pub fn take_handed(&mut self) {
        // A caught download by its own setting, the panel's and menus' by
        // theirs.
        let caught = dm_core::lock(&self.backend.handed)
            .front()
            .is_some_and(|r| r.caught);
        let ask = !self.headless
            && if caught {
                self.applied.ask_caught_downloads
            } else {
                self.applied.ask_panel_downloads
            };
        if ask && (self.dialogs.file_info.is_some() || self.dialogs.links.is_some()) {
            return;
        }
        // A row waiting for a fresh link takes the next download caught in
        // the browser as it, rather than a row of its own.
        if let Some((id, since)) = self.awaiting_link {
            if since.elapsed() > AWAIT_LINK_FOR {
                self.stop_awaiting_link();
            } else {
                let next = dm_core::lock(&self.backend.handed)
                    .front()
                    .is_some_and(|r| r.links.is_empty());
                if next {
                    let request = dm_core::lock(&self.backend.handed).pop_front();
                    if let Some(request) = request {
                        self.take_fresh_link(id, &request.url);
                    }
                    return;
                }
            }
        }
        let Some(request) = dm_core::lock(&self.backend.handed).pop_front() else {
            return;
        };
        // The same address handed over again within a few seconds is one
        // click, not two: a double-click on a link in the browser saved
        // `setup (1).deb` and `setup (2).deb`.
        if request.links.is_empty() && self.just_taken(&request.url) {
            return;
        }
        // A page's links: the "Download all links" dialog, or straight
        // into the main queue with nobody to ask.
        if !request.links.is_empty() {
            if ask {
                let backend = self.backend.clone();
                self.dialogs.open_links(request, &backend);
            } else {
                let with = dm_core::AddOptions {
                    later: true,
                    queue: dm_core::MAIN_QUEUE.into(),
                    referrer: request.referrer.clone(),
                    ..Default::default()
                };
                // Each link's words on the page as its description, as the
                // dialog does by default.
                let items = request
                    .links
                    .iter()
                    .map(|url| {
                        let own = dm_core::AddOptions {
                            description: request.texts.get(url).cloned().unwrap_or_default(),
                            // The name its `download` attribute gives.
                            save_name: request.names.get(url).cloned(),
                            ..with.clone()
                        };
                        (url.clone(), own)
                    })
                    .collect();
                if let Err(e) = self.add_items(items, &HashMap::new()) {
                    self.said = e.into();
                }
            }
            return;
        }
        // A download caught in the browser for an address already listed is
        // not fetched a second time unasked: the person is shown the row and
        // chooses "Download again". Only with a window to ask in.
        if request.caught && !self.headless {
            if let Some(id) = crate::dialogs::worth_keeping(self, &request.url) {
                // Answered at once where the person ticked "Remember my choice".
                match crate::dialogs::Dup::from_word(&self.applied.duplicate_choice) {
                    Some(how) => {
                        crate::dialogs::answer_duplicate(self, request.url.clone(), id, how)
                    }
                    None => self.dialogs.duplicate = Some((request.url.clone(), id)),
                }
                self.taken_from_browser(&request.url);
                return;
            }
        }
        if !ask {
            match self
                .backend
                .add_with(&request.url, dm_shell::shell::add_options(&request))
            {
                Ok(_) => self.taken_from_browser(&request.url),
                Err(e) => self.said = e.into(),
            }
        } else {
            self.dialogs.open_handed(request);
        }
    }

    /// `--when-done` from the command line, left in the inbox by this
    /// launch or a later one.
    /// `--new-download`, left in the inbox by this launch or a later one:
    /// the window comes forward with Add URL open.
    pub fn take_new_download(&mut self) {
        let file = dm_shell::host::inbox(&dm_shell::settings::home())
            .join(dm_shell::shell::NEW_DOWNLOAD_FILE);
        if std::fs::remove_file(&file).is_ok() && !self.headless {
            self.show_asked = true;
            self.run(Command::AddUrl);
        }
    }

    pub(super) fn take_when_done(&mut self) {
        let file = dm_shell::host::inbox(&dm_shell::settings::home())
            .join(dm_shell::shell::WHEN_DONE_FILE);
        let Ok(word) = std::fs::read_to_string(&file) else {
            return;
        };
        let _ = std::fs::remove_file(&file);
        let command = match word.trim() {
            "nothing" => Command::WhenDoneNothing,
            "exit" => Command::WhenDoneQuit,
            "sleep" => Command::WhenDoneSleep,
            "hibernate" => Command::WhenDoneHibernate,
            "shutdown" => Command::WhenDoneShutDown,
            _ => return,
        };
        use crate::power::WhenDone;
        let wanted = match command {
            Command::WhenDoneSleep => WhenDone::Sleep,
            Command::WhenDoneHibernate => WhenDone::Hibernate,
            Command::WhenDoneShutDown => WhenDone::ShutDown,
            _ => WhenDone::Nothing,
        };
        // As the menu greys them: only what this computer can do.
        if wanted == WhenDone::Nothing || crate::power::available(wanted) {
            self.run(command);
        } else {
            self.said = crate::i18n::Said::new(
                "This computer has no way to do that when the downloads are done.",
                &[],
            );
        }
    }

    /// Starts waiting for a fresh link for row `id`, from its page opened in
    /// the browser.
    pub fn await_link(&mut self, id: Id) {
        self.awaiting_link = Some((id, Instant::now()));
        dm_shell::shell::route_to_window(true);
    }

    /// Stops waiting for a fresh link.
    pub fn stop_awaiting_link(&mut self) {
        self.awaiting_link = None;
        dm_shell::shell::route_to_window(false);
    }

    /// The download caught in the browser becomes row `id`'s link, and the
    /// row goes on from where it stopped.
    pub(super) fn take_fresh_link(&mut self, id: Id, url: &str) {
        self.stop_awaiting_link();
        let failed = self
            .rows
            .iter()
            .find(|r| r.id == id)
            .is_some_and(|r| matches!(r.state, State::Failed(_)));
        match self.backend.set_url(id, url) {
            Ok(()) => {
                if failed {
                    self.backend.retry(id);
                }
                self.said = crate::i18n::Said::new(
                    "The new link was taken from the browser; the download goes on.",
                    &[],
                );
            }
            Err(e) => self.said = e.into(),
        }
        self.refresh();
    }

    /// While a fresh link is awaited: what to do, and a way out.
    pub(super) fn awaiting_bar(&mut self, ui: &mut egui::Ui) {
        let Some((id, _)) = self.awaiting_link else {
            return;
        };
        let name = self
            .rows
            .iter()
            .find(|r| r.id == id)
            .map(|r| name_of(r))
            .unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.label(tf(
                "Click the download again on its page in your browser, and dm uses that as the new link for {}.",
                &[&crate::bidi::visual(&name)],
            ));
            if ui.button(t("Cancel")).clicked() {
                self.stop_awaiting_link();
            }
        });
        ui.separator();
    }

    /// A download the browser handed over was added: one handed over again
    /// in the next few seconds is the same click.
    pub fn taken_from_browser(&mut self, url: &str) {
        self.recent_hands.push((url.to_string(), Instant::now()));
    }

    /// Whether `url` was taken from the browser a moment ago. One handed
    /// over and cancelled is not remembered: clicked again, it is meant.
    pub(super) fn just_taken(&mut self, url: &str) -> bool {
        let now = Instant::now();
        self.recent_hands
            .retain(|(_, at)| now.duration_since(*at) < HANDED_TWICE_WITHIN);
        self.recent_hands.iter().any(|(taken, _)| taken == url)
    }

    /// Runs a countdown's action once its time is up.
    pub fn countdown_tick(&mut self) {
        if let Some((action, at)) = self.countdown {
            if Instant::now() >= at {
                self.countdown = None;
                if action == crate::power::WhenDone::Quit {
                    self.quit_asked = true;
                    return;
                }
                // In the list of problems as well as the status line: whoever
                // set it is likely away, and a status line is overwritten.
                // A refusal that comes back later is left for the list too,
                // as anything running apart from the window does.
                let notices = self.backend.notices();
                let report: crate::power::Report =
                    Box::new(move |why| dm_core::lock(&notices).push(why));
                if let Err(e) = self.backend.platform.power(action, report) {
                    self.problem(e.clone());
                    self.said = e.into();
                }
            }
        }
    }

    /// What runs whether or not the window shows: keeping the list current
    /// and watching the clipboard.
    pub fn logic(&mut self, ctx: &egui::Context) {
        let (read_now, look_again) = refresh_plan(
            QUEUE_CHANGED.load(std::sync::atomic::Ordering::Acquire),
            self.refreshed.map(|at| at.elapsed()),
        );
        if read_now {
            self.refresh();
        }
        // A download handed over and waiting for the person's answer (Download
        // details, the page's links, "Download again") has left the browser
        // already: a window behind another, or in the tray, made it look lost.
        let waiting = |app: &Self| {
            (
                app.dialogs.file_info.is_some(),
                app.dialogs.links.is_some(),
                app.dialogs.duplicate.is_some(),
            )
        };
        // What was held back while the person was busy elsewhere (a
        // fullscreen app, Focus Assist), said once they are not.
        self.backend.platform.tick();
        self.big_download_hint();
        self.details_hint();
        let before = waiting(self);
        self.take_handed();
        let after = waiting(self);
        if !self.headless && (after.0 && !before.0 || after.1 && !before.1 || after.2 && !before.2)
        {
            self.show_asked = true;
        }
        self.countdown_tick();
        self.delete_due_files();
        if let Some(why) = dm_core::lock(&self.opener_said).take() {
            self.said = why.into();
        }
        let hung_up = dm_core::lock(&self.hung_up).take();
        if let Some(hung) = hung_up {
            self.say_hung_up(hung);
        }
        if let Some((checked, changed)) = dm_core::lock(&self.rechecked).take() {
            self.said = crate::i18n::Said::counting(
                "Checked {} finished file with its server. Changed since, and downloaded again: {}.||Checked {} finished files with their servers. Changed since, and downloaded again: {}.",
                checked as u64,
                &[&checked, &changed],
            );
        }
        // Here, not in the drawing: saved searches run on their timer while
        // dm is in the tray or headless, when nothing is drawn.
        self.tick_searches();
        self.take_watched();
        if self.clipboard.on
            && self
                .clipboard
                .polled
                .is_none_or(|at| at.elapsed() >= CLIPBOARD)
        {
            self.check_clipboard();
        }
        self.answer_control(ctx);
        self.weekly_update(ctx);
        // Four times a second only while something moves: idle, in the
        // tray with four finished rows, dm drew itself four times a second
        // and spent a third of a core doing it. A hand-off, a
        // control request and a tray click wake it at once regardless.
        let moving = self.countdown.is_some()
            || self
                .rows
                .iter()
                .any(|r| matches!(r.state, State::Running | State::Queued));
        // Hidden in the tray, no one sees a frame: woken each second while
        // something moves or the clipboard is watched (When done, the
        // complete window, a new link). With nothing moving, a change to
        // the queue wakes it, visible or hidden, and the rest is every 30
        // seconds, which the saved searches' minutes allow.
        let wake = match (self.hidden, moving || self.clipboard.on) {
            (true, true) => HIDDEN_REFRESH,
            (true, false) => HIDDEN_IDLE,
            (false, _) if moving => REFRESH,
            (false, _) if self.clipboard.on => CLIPBOARD_REFRESH,
            (false, _) => IDLE_REFRESH,
        };
        // A finished torrent being shared says how much it has sent, each second it is
        // seen (gaps row 147); hidden, no one reads it.
        let wake = if !self.hidden && dm_core::sharing_any() {
            wake.min(REFRESH.max(Duration::from_secs(1)))
        } else {
            wake
        };
        // A watched folder is read every few seconds, hidden or not.
        let wake = if self.applied.watch_folder.trim().is_empty() {
            wake
        } else {
            wake.min(WATCH_EVERY)
        };
        // A change since the last read is read as soon as it may be, and
        // not left to the long wait a list of nothing moving earns.
        let wake = look_again.map_or(wake, |again| wake.min(again));
        ctx.request_repaint_after(wake);
    }

    /// The weekly update check: about once a week, where turned on,
    /// a look for a newer dm, and About opened to offer it when there is one.
    /// Nothing is installed without the person.
    pub(super) fn weekly_update(&mut self, ctx: &egui::Context) {
        // Due again a week after the last, however long dm has been running
        // in the tray: the time of the last check is what decides.
        if self.applied.check_updates {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            if crate::update::due(self.applied.update_checked, now) {
                self.weekly_pending = true;
                crate::update::check(
                    &self.backend.runtime,
                    self.client(),
                    Arc::clone(&self.update),
                    ctx.clone(),
                );
                let mut wanted = self.applied.clone();
                wanted.update_checked = now;
                if let Err(e) = self.apply_settings(wanted) {
                    self.said = e.into();
                    // Not saved, but not asked again every frame either.
                    self.applied.update_checked = now;
                }
            }
        }
        if self.weekly_pending && matches!(*dm_core::lock(&self.update), Update::Newer(..)) {
            self.weekly_pending = false;
            self.dialogs.about_open = true;
        }
    }

    /* --------------------------------------------------- control protocol */

    /// Puts settings into force, and puts the window's copy back when that
    /// fails rather than leaving it showing what is not in force.
    pub fn apply_settings(&mut self, mut wanted: Settings) -> Result<(), String> {
        // What makes dm run something is a person's to set, as a settings file imported never
        // sets it (`Settings::keep_programs_of`).
        if self.by_script() {
            let kept = wanted.keep_programs_of(&self.applied);
            let said = if kept.command {
                Some("The command to run when a download finishes was not changed: a script was driving the window a moment ago; set it again")
            } else if kept.schedule {
                Some("The program to run when the scheduled downloads are done was not changed: a script was driving the window a moment ago; set it again")
            } else if kept.proxy_script {
                Some("The proxy script was not changed: a script was driving the window a moment ago; set it again")
            } else {
                None
            };
            if let Some(said) = said {
                self.said = said.to_string().into();
            }
        }
        let watch_changed = wanted.watch_clipboard != self.applied.watch_clipboard;
        match self.backend.set_settings(wanted) {
            Ok(now) => {
                crate::i18n::set(crate::i18n::chosen(&now.language));
                // A search saved or changed is looked at on the next frame,
                // not in a minute: one saved to run every hour runs now.
                if self.applied.site_searches != now.site_searches {
                    self.searches_checked = None;
                }
                self.applied = now;
                // Categories of a person's own may have changed.
                self.place_rows();
                if watch_changed {
                    self.set_clipboard_watch(self.applied.watch_clipboard);
                }
                Ok(())
            }
            Err(e) => {
                self.applied = self.backend.settings();
                Err(e)
            }
        }
    }

    /* ----------------------------------------------------------- places */

    /// Takes rows out of the list, with their staged parts; finished files
    /// stay on disk.
    pub fn remove_now(&mut self, ids: &[Id]) {
        self.backend.remove_many(ids);
        // What the status line said may have named one of them.
        self.said = Default::default();
        for id in ids {
            self.dialogs.forget(*id);
        }
        self.selected.clear();
        self.refresh();
    }

    /// "Delete file after": each finished row whose time has come has its
    /// file moved to the Trash and is removed. A file that could not be
    /// moved stays, with its row, and is said; the row is not tried again.
    pub fn delete_due_files(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let due: Vec<Entry> = self
            .rows
            .iter()
            .filter(|r| r.delete_due_in(now) == Some(0))
            .map(|r| Entry::clone(r))
            .collect();
        if due.is_empty() {
            return;
        }
        let mut gone = Vec::new();
        let mut failed = Vec::new();
        for row in due {
            // Its own file alone: one moved away or since written by another
            // row is not this row's to delete.
            let present = !row.path.is_empty() && !crate::table::gone(&row);
            // A finished torrent still shared lets go of its file first (gaps row 147).
            match present.then(|| {
                dm_core::stop_sharing(std::path::Path::new(&row.path));
                self.backend.platform.trash(&row.path)
            }) {
                Some(Err(e)) => {
                    failed.push(format!("{}: {e}", row.path));
                    self.backend.set_delete_after(row.id, None);
                }
                _ => gone.push(row.id),
            }
        }
        if !gone.is_empty() {
            self.backend.remove_many(&gone);
            for id in &gone {
                self.dialogs.forget(*id);
                self.selected.remove(id);
            }
        }
        if !failed.is_empty() {
            self.said = crate::i18n::Said::new("Could not delete {}", &[&failed.join("; ")]);
        }
        self.refresh();
    }

    /// A very big download that has started: said once, ever, that the
    /// Scheduler can run it at night. Not said again once it has been.
    pub fn big_download_hint(&mut self) {
        const BIG: u64 = 5 << 30;
        if self.applied.hinted_big
            || self.headless
            || !self
                .rows
                .iter()
                .any(|r| r.state == State::Running && r.progress.total.is_some_and(|t| t >= BIG))
        {
            return;
        }
        self.said = crate::i18n::Said::new(
            "This is a big download. Downloads › Scheduler can run it at night, when you are not using the computer.",
            &[],
        );
        let mut wanted = self.applied.clone();
        wanted.hinted_big = true;
        if let Err(e) = self.apply_settings(wanted) {
            self.said = e.into();
        }
    }

    /// A download that has started with no progress window to show it: said
    /// once, ever, where to open one. The progress window no longer opens by
    /// itself, and without the words a person has nothing to say it is there.
    /// Waits while something else is being said.
    pub fn details_hint(&mut self) {
        if self.applied.hinted_details
            || self.applied.show_progress
            || self.headless
            || !self.said.is_empty()
            || !self.rows.iter().any(|r| r.state == State::Running)
        {
            return;
        }
        self.said = crate::i18n::Said::new(
            "Hover a download and press Details to watch its progress.",
            &[],
        );
        let mut wanted = self.applied.clone();
        wanted.hinted_details = true;
        if let Err(e) = self.apply_settings(wanted) {
            self.said = e.into();
        }
    }

    /// Takes finished rows off the list with no question, since their files
    /// stay where they were saved, and keeps them for a few seconds so Undo
    /// can put them back.
    pub fn remove_finished_with_undo(&mut self, ids: &[Id]) {
        let rows = self.backend.finished_rows(ids);
        self.remove_now(ids);
        if !rows.is_empty() {
            self.said = crate::i18n::Said::counting(
                "Removed {} download from the list.||Removed {} downloads from the list.",
                rows.len() as u64,
                &[&rows.len()],
            );
            self.undo_remove = Some((Instant::now(), rows));
        }
    }

    /// Puts back what was just taken off the list, if it is still in time.
    pub fn undo_remove(&mut self) {
        let Some((at, rows)) = self.undo_remove.take() else {
            return;
        };
        if at.elapsed() >= UNDO_FOR || rows.is_empty() {
            return;
        }
        let ids: Vec<Id> = rows.iter().map(|(_, e)| e.id).collect();
        let n = rows.len();
        self.backend.put_back(rows);
        self.refresh();
        self.selected = ids.into_iter().collect();
        self.said = crate::i18n::Said::counting(
            "Put back {} download.||Put back {} downloads.",
            n as u64,
            &[&n],
        );
    }

    /// Program › Exit, Cmd+Q and the tray's Quit: asked first while
    /// anything is downloading, since quitting stops it. A window with
    /// nobody to ask (headless) quits.
    pub fn ask_quit(&mut self) {
        if self.headless || self.downloading() == 0 {
            self.quit_asked = true;
        } else {
            self.dialogs.confirm_exit = true;
        }
    }

    /// Clears a sentence about one row once that row has run again and
    /// stopped, or is gone: "Trying … again with the sign-in." stood on
    /// after the download it spoke of had long finished.
    pub(super) fn forget_said_about(&mut self) {
        let Some((id, said, seen_running)) = self.said_about.as_mut() else {
            return;
        };
        if *said != self.said {
            self.said_about = None;
            return;
        }
        let state = self.rows.iter().find(|r| r.id == *id).map(|r| &r.state);
        match state {
            Some(State::Running | State::Queued) => *seen_running = true,
            Some(_) if !*seen_running => {}
            _ => {
                self.said = crate::i18n::Said::default();
                self.said_about = None;
            }
        }
    }

    /// How many downloads are running, and how many are in line to start.
    pub fn running_and_waiting(&self) -> (usize, usize) {
        let count = |state: State| self.rows.iter().filter(|r| r.state == state).count();
        (count(State::Running), count(State::Queued))
    }

    /// How many downloads are running or about to.
    pub fn downloading(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| matches!(r.state, State::Running | State::Queued))
            .count()
    }

    /// A command's name as its menu item and button show it now: Resume on
    /// rows that failed is a retry, and says so.
    pub fn title_of(&self, command: Command) -> &'static str {
        if command == Command::DownloadNow && self.failed_only() {
            return t("Retry");
        }
        // A row the schedule or the download limit holds is not resumed but
        // started outside it, as its progress window says.
        if command == Command::DownloadNow && self.held_only() {
            return t("Start now");
        }
        command.title()
    }

    /// Whether every selected row is held by the schedule or the limit.
    pub(super) fn held_only(&self) -> bool {
        let targets = self.targets();
        !targets.is_empty()
            && targets
                .iter()
                .all(|r| r.state == State::Paused && r.paused_by_schedule)
    }

    /// Whether every selected row failed, so Resume would retry each.
    pub(super) fn failed_only(&self) -> bool {
        let targets = self.targets();
        !targets.is_empty() && targets.iter().all(|r| matches!(r.state, State::Failed(_)))
    }

    /// Why held rows wait, and until when: the download limit's time left,
    /// or when the schedule next opens. `None` when nothing holds them.
    pub fn held_because(&self) -> Option<String> {
        if let Some(wait) = self.backend.queue.allowance_spent() {
            return Some(tf(
                "Held by the download limit; it goes on by itself in {}.",
                &[&format::span(wait)],
            ));
        }
        let schedule = &self.applied.schedule;
        if schedule.enabled && !schedule.open() {
            return Some(match opens_next(schedule) {
                Some(at) => tf("Held by the schedule; it starts by itself at {}.", &[&at]),
                None => t("Held by the schedule.").to_string(),
            });
        }
        None
    }

    /// Adds downloads, as "Start now" and "Add for later" do.
    pub fn add(&mut self, urls: &[String], with: dm_core::AddOptions) -> Result<usize, String> {
        self.add_sized(urls, &with, &HashMap::new())
    }

    /// The same, with the sizes a dialog already learned for some of the
    /// links, shown on their rows until each learns its own.
    pub fn add_sized(
        &mut self,
        urls: &[String],
        with: &dm_core::AddOptions,
        sizes: &HashMap<String, u64>,
    ) -> Result<usize, String> {
        let items = urls.iter().map(|u| (u.clone(), with.clone())).collect();
        self.add_items(items, sizes)
    }

    /// Adds downloads, each with options of its own (a name, a description,
    /// a sign-in), in one batch.
    pub fn add_items(
        &mut self,
        items: Vec<(String, dm_core::AddOptions)>,
        sizes: &HashMap<String, u64>,
    ) -> Result<usize, String> {
        let mut added = 0;
        let mut last_error = None;
        self.last_added.clear();
        let count = items.len();
        let asked: Vec<(String, bool)> = items.iter().map(|(u, w)| (u.clone(), w.later)).collect();
        for ((url, later), outcome) in asked.iter().zip(self.backend.add_each(items)) {
            match outcome {
                Ok(id) => {
                    added += 1;
                    self.last_added.push(id);
                    self.own.insert(id);
                    if let Some(size) = sizes.get(url) {
                        self.sizes_told.insert(id, *size);
                    }
                    if !later && self.applied.show_progress && count == 1 {
                        self.dialogs.progress.insert(id);
                    }
                }
                Err(e) => last_error = Some(e),
            }
        }
        self.refresh();
        let failed = count.saturating_sub(added);
        match (added, last_error) {
            (0, Some(e)) => Err(e),
            (_, Some(e)) => {
                // The rest went in; the ones that did not are said, not lost.
                self.problem(match failed {
                    1 => format!("One of the links was not added: {e}"),
                    n => format!("{n} of the links were not added: {e}"),
                });
                Ok(added)
            }
            _ => Ok(added),
        }
    }

    /* ---------------------------------------------------------- drawing */
}

/// A question a row raised, waiting for the dialogs to be free.
pub(crate) enum Ask {
    /// The server sent another name than the address's.
    Name(crate::dialogs::NameAsk),
    /// A web page answered for a file.
    Page(Id),
    /// A server said the file is not there.
    NotFound(Id),
}

impl DmApp {
    /// Looks at the rows as just read for what reference A asks about
    /// (maintainer, 2026-11-01): a name the server sent that is not the
    /// address's, a web page where a file was asked for, a 404. Only what is
    /// new since the last read is asked about, and the first read only
    /// notes what is there: rows of an earlier run are not asked about.
    pub(super) fn watch_asks(&mut self) {
        let first = !self.asks_primed;
        self.asks_primed = true;
        let mut seen = std::mem::take(&mut self.ask_seen);
        // Several rows new at once are a batch (Download all links), which
        // is not asked about names one by one.
        let fresh = self
            .rows
            .iter()
            .filter(|r| !seen.contains_key(&r.id))
            .count();
        for row in &self.rows {
            let failed = match &row.state {
                State::Failed(why) => Some(why.as_str()),
                _ => None,
            };
            let before = seen.insert(row.id, (row.filename.clone(), failed.is_some()));
            if first {
                continue;
            }
            let was_unnamed = before.as_ref().is_none_or(|(name, _)| name.is_empty());
            let chosen = row
                .save_name
                .as_deref()
                .is_some_and(|name| !name.trim().is_empty());
            // A download the browser handed over comes with the name the browser
            // gives it (and has a page it came from): not asked about. Nor one a
            // person has said yes to saving as an HTML page, which is named so.
            let typed_in = row.referrer.is_empty() && !row.accept_page;
            if was_unnamed && !row.filename.is_empty() && !chosen && typed_in && fresh < 2 {
                let address_name = crate::format::provisional(&row.url);
                if row.filename != address_name
                    && without_copy_number(&row.filename) != address_name
                {
                    self.asks.push_back(Ask::Name(crate::dialogs::NameAsk {
                        id: row.id,
                        address_name,
                        server_name: row.filename.clone(),
                    }));
                }
            }
            let newly_failed = before.as_ref().is_none_or(|(_, was)| !*was);
            if let (true, Some(why)) = (newly_failed, failed) {
                if why.contains("answered with a web page rather than the file") && !row.accept_page
                {
                    self.asks.push_back(Ask::Page(row.id));
                } else if why.contains("answered 404") {
                    self.asks.push_back(Ask::NotFound(row.id));
                }
            }
        }
        let ids: HashSet<Id> = self.rows.iter().map(|r| r.id).collect();
        seen.retain(|id, _| ids.contains(id));
        self.ask_seen = seen;
        self.promote_asks();
    }

    /// Opens the next waiting question, when no other is being asked: one at
    /// a time, in the order they came, and none for a row that is gone.
    pub(crate) fn promote_asks(&mut self) {
        if self.dialogs.asking() {
            return;
        }
        while let Some(ask) = self.asks.pop_front() {
            match ask {
                Ask::Name(n) if self.row(n.id).is_some() => {
                    self.dialogs.name_ask = Some(n);
                    return;
                }
                Ask::Page(id) if self.row(id).is_some() => {
                    self.dialogs.page_ask = Some(id);
                    return;
                }
                Ask::NotFound(id) if self.row(id).is_some() => {
                    self.dialogs.not_found = Some(id);
                    return;
                }
                _ => {}
            }
        }
    }

    /// A person said no to the server's name: the file is given the
    /// address's, at once if it is finished, else as it finishes.
    pub(crate) fn use_address_name(&mut self, id: Id, name: &str) {
        match self.row(id).map(|r| r.state.clone()) {
            Some(State::Completed) => {
                let _ = self.backend.rename(id, name);
            }
            Some(State::Failed(_) | State::Cancelled) | None => {}
            Some(_) => {
                self.pending_renames.insert(id, name.to_string());
            }
        }
    }

    /// Renames what finished since a person said no to the server's name.
    pub(super) fn apply_pending_renames(&mut self) {
        let ids: Vec<Id> = self.pending_renames.keys().copied().collect();
        for id in ids {
            match self.row(id).map(|r| r.state.clone()) {
                Some(State::Completed) => {
                    if let Some(name) = self.pending_renames.remove(&id) {
                        let _ = self.backend.rename(id, &name);
                    }
                }
                Some(State::Failed(_) | State::Cancelled) | None => {
                    self.pending_renames.remove(&id);
                }
                Some(_) => {}
            }
        }
    }
}

/// `name` without the " (2)" dm puts before the extension to keep two copies
/// of one file apart: that number is dm's, not something the server sent.
fn without_copy_number(name: &str) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    if let Some(open) = stem.rfind(" (") {
        if let Some(digits) = stem[open + 2..].strip_suffix(')') {
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                return format!("{}{}", &stem[..open], ext);
            }
        }
    }
    name.to_string()
}

#[cfg(test)]
mod copy_number_tests {
    use super::without_copy_number;

    #[test]
    fn the_number_dm_adds_to_a_second_copy_is_not_the_servers() {
        assert_eq!(without_copy_number("again (2).bin"), "again.bin");
        assert_eq!(without_copy_number("again (12)"), "again");
        assert_eq!(
            without_copy_number("Report (final).pdf"),
            "Report (final).pdf"
        );
        assert_eq!(without_copy_number("a.b (1).tar.gz"), "a.b (1).tar.gz");
        assert_eq!(without_copy_number("plain.bin"), "plain.bin");
    }
}
