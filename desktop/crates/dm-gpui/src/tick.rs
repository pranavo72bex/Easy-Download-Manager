// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the GPUI window does on each wake beside reading the list, as the
//! egui window's tick does it (`docs/architecture/ARCHITECTURE.md`): orders
//! left by the command line, what the read detected (notifications, sounds,
//! a hand-off's address listed again), the schedules' hooks and When done,
//! the browser's hand-offs, the When done countdown, "Delete file after",
//! the watched folder and the one-time hints. Each rule is the
//! application's (`dm_shell::app`); the window says what came of it and
//! opens what it asks in.

use std::time::Instant;

use dm_desktop_base::i18n::{t, tf};
use dm_shell::app::session::Fired;
use gpui::Context;

use crate::list::Downloads;

impl Downloads {
    /// One wake's work after the read (`Downloads::read`), on the main
    /// window's thread.
    pub(crate) fn tick(&mut self, cx: &mut Context<Self>) {
        // Windows' High Contrast turned on, off or to another theme: every
        // window drawn again in its colours (`theme.rs`), not only the one in
        // front when it next draws.
        let contrast = dm_desktop_base::contrast::system();
        if contrast != self.contrast {
            self.contrast = contrast;
            cx.refresh_windows();
        }
        let home = dm_shell::settings::home();
        // `--new-download` and `--when-done`, left in the inbox by this
        // launch or a later one.
        if dm_shell::app::session::new_download_asked(&home) {
            crate::addurl::open(cx.entity().downgrade(), cx);
        }
        if let Some(wanted) = dm_shell::app::session::when_done_asked(&home) {
            // As the menu greys them: only what this computer can do.
            if wanted.possible(dm_desktop_base::power::available) {
                self.run(dm_shell::app::commands::when_done_command(wanted), cx);
            } else {
                self.say(
                    t("This computer has no way to do that when the downloads are done.").into(),
                );
            }
        }
        self.told(cx);
        self.schedule_hooks();
        self.this_done_due();
        if self.when_queue_dry() {
            self.quit(cx);
            return;
        }
        // What a platform held back while the person was busy (a
        // fullscreen app) is said once they are not.
        self.backend.platform.tick();
        self.big_download_hint();
        self.take_handed(cx);
        self.countdown_tick(cx);
        self.delete_due_files();
        self.take_watched(cx);
        self.say_rechecked();
        self.take_searches(cx);
        self.weekly_update(cx);
        self.check_clipboard(cx);
        // A second launch, the browser's button or the Dock asked for the
        // window; the tray says how the downloads go.
        if self.take_show_asked() {
            // A browser installed since dm started is registered now, so "open dm once" works
            // while dm runs.
            if let Some(words) = dm_shell::register::ensure_and_say() {
                self.backend.note(words);
            }
            self.show_main(cx);
        }
        self.tray_follow();
        // Links dragged in from a browser, and the drop target as Options says.
        self.take_dropped_links(cx);
        self.follow_drop_target(cx);
        self.refresh_menus(cx);
        let problems = self.backend.problems();
        self.problems.extend(
            problems
                .iter()
                .map(|p| dm_desktop_base::format::worded_problem(p)),
        );
    }

    /// What the reads since the last tick detected: a notification for each
    /// download that failed, one for what finished (a batch is one
    /// sentence; a single file keeps its Open), the sounds chosen, and a
    /// hand-off's address already listed said.
    fn told(&mut self, cx: &mut Context<Self>) {
        let detected = std::mem::take(&mut self.detected);
        let settings = self.backend.settings();
        self.prune_progress(cx);
        self.say_opener();
        let mut finished: Vec<(String, String)> = Vec::new();
        let mut finished_ids: Vec<dm_core::Id> = Vec::new();
        // Each failure's name and why, said aloud too (gaps row U23).
        let mut failures: Vec<(String, String)> = Vec::new();
        let expired = detected
            .iter()
            .find_map(|read| read.expired.first().copied());
        self.offer_new_link(expired, cx);
        for read in detected {
            for row in &self.rows {
                if read.failed.contains(&row.id) {
                    let (name, why) = (
                        dm_shell::app::names::name_of(row),
                        dm_desktop_base::rows::status_of(row),
                    );
                    // Told as a finish is, with why in plain words.
                    self.backend.platform.notify_failure(
                        t("Download failed"),
                        &format!("{name} — {why}"),
                        row.id,
                    );
                    failures.push((name, why));
                }
                if read.finished.contains(&row.id) {
                    finished.push((dm_shell::app::names::name_of(row), row.path.clone()));
                    finished_ids.push(row.id);
                }
            }
            // A download that started without the window starting it (a
            // browser's hand-off, a second launch) opens its progress window
            // as one started here does, where that is asked for: one window
            // for one download, not a stack for a batch.
            if settings.show_progress {
                if let [only] = read.started_elsewhere.as_slice() {
                    if !self.progress_closed_has(*only) {
                        self.show_progress(*only, cx);
                    }
                }
            }
            // A browser's hand-off of an address already listed: added by
            // the inbox before the window heard of it, and the browser has
            // let its own download go. Said, as Add URL would have asked.
            if let Some(row) = read
                .added_again
                .and_then(|id| self.rows.iter().find(|r| r.id == id))
            {
                self.say(tf(
                    "Added {} again: that address was already in the list.",
                    &[&dm_shell::app::names::name_of(row)],
                ));
            }
        }
        if !finished_ids.is_empty() {
            self.finished(&finished_ids, cx);
        }
        // Said aloud from the status bar, not by the system's notification
        // alone, which a screen reader may not read (gaps row U23).
        let names: Vec<String> = finished.iter().map(|(name, _)| name.clone()).collect();
        if let Some(sentence) = crate::a11y::heard(&names, &failures) {
            self.heard = Some((sentence, std::time::Instant::now()));
        }
        match finished.as_slice() {
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
        let sound = |event: &str| {
            settings
                .sounds
                .get(event)
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
        };
        if settings.sound_on_complete && !finished.is_empty() {
            match sound("complete") {
                Some(path) => self.backend.platform.play_file(&path),
                None => self.backend.platform.chime(),
            }
        }
        if settings.sound_on_failure && !failures.is_empty() {
            match sound("failed") {
                Some(path) => self.backend.platform.play_file(&path),
                None => self.backend.platform.alert(),
            }
        }
    }

    /// The schedules, after every read (`Session::schedule_tick`): a
    /// once-only date that has passed is put away, and what the schedule
    /// armed for When done is said.
    fn schedule_hooks(&mut self) {
        let settings = self.backend.settings();
        if let Some(wanted) = dm_desktop_base::held::spent_once_now(&settings) {
            if let Err(why) = self.backend.set_settings(wanted) {
                self.problems.push(why);
            }
        }
        let settings = self.backend.settings();
        let tick = self.session.schedule_tick(
            &self.backend,
            &settings,
            &self.rows,
            dm_desktop_base::power::available,
        );
        if let Some(wanted) = tick.armed {
            self.say(tf(
                "The schedule has set what dm does when the downloads are done: {}.",
                &[&t(wanted.verb())],
            ));
        }
        self.problems.extend(tick.problems);
        // Scheduler › Hang up modem when done: off the window's thread, as Windows takes a
        // moment to end a connection; what it came to is said when it is known.
        if tick.hang_up && cfg!(windows) {
            if let Some(hung) =
                dm_shell::dialup::hang_up_later(std::sync::Arc::clone(&self.hung_up))
            {
                self.say_hung_up(hung);
            }
        }
        let hung = dm_core::lock(&self.hung_up).take();
        if let Some(hung) = hung {
            self.say_hung_up(hung);
        }
    }

    /// Says what hanging up came to: a connection that was not up needs no word, one
    /// Windows would not end is a problem.
    fn say_hung_up(&mut self, hung: dm_shell::dialup::HungUp) {
        match hung_up_words(hung) {
            Some(Ok(sentence)) => self.say(sentence),
            Some(Err(problem)) => self.problems.push(problem),
            None => {}
        }
    }

    /// Each download whose own "When this download is done" is due (gaps row U36): its file or
    /// folder opened, or exit, sleep, hibernate or turn off through When done's own countdown,
    /// which can be cancelled.
    fn this_done_due(&mut self) {
        use dm_shell::app::session::ThisDone;
        for (row, choice) in self.session.this_done_due(&self.rows) {
            let opened = match choice {
                ThisDone::Nothing => Ok(()),
                ThisDone::Open => self.backend.platform.open(&row.path),
                ThisDone::OpenFolder => self.backend.platform.reveal(&row.path),
                ThisDone::Then(action) => {
                    let at = Instant::now()
                        + std::time::Duration::from_secs(dm_shell::app::power::COUNTDOWN_SECS);
                    self.session.countdown = Some((action, at));
                    Ok(())
                }
            };
            if let Err(why) = opened {
                self.problems.push(why);
            }
        }
    }

    /// A queue that has run dry has finished: its own sound, where the
    /// person chose a file, and When done (`Session::when_done_fires`).
    /// Whether dm is to quit now.
    fn when_queue_dry(&mut self) -> bool {
        if !self.session.ran_dry(&self.rows) {
            return false;
        }
        let settings = self.backend.settings();
        if let Some(path) = settings
            .sounds
            .get("queue_done")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
        {
            self.backend.platform.play_file(&path);
        }
        self.session.when_done_fires(false, Instant::now()) == Fired::QuitNow
    }

    /// A very big download that has started: said once, ever, that the
    /// Scheduler can run it at night.
    pub(crate) fn big_download_hint(&mut self) {
        use dm_shell::app::hints::Hint;
        let settings = self.backend.settings();
        if !Hint::BigDownload.due(&settings, &self.rows, self.said.is_none()) {
            return;
        }
        self.say(t("This is a big download. Downloads › Scheduler can run it at night, when you are not using the computer.").into());
        let mut wanted = settings;
        Hint::BigDownload.mark(&mut wanted);
        if let Err(e) = self.backend.set_settings(wanted) {
            self.say(e);
        }
    }

    /// A download handed over by the browser, routed by the application
    /// (`Hands::next`): Download details for it, the question for an
    /// address already listed, or added straight in.
    fn take_handed(&mut self, cx: &mut Context<Self>) {
        use dm_shell::app::handoff::{Asking, Route};
        let asking = if self.handed_open {
            Asking::Busy
        } else {
            Asking::Free
        };
        let settings = self.backend.settings();
        let route =
            self.session
                .hands
                .next(&self.backend, &settings, &self.rows, asking, Instant::now());
        let me = cx.entity().downgrade();
        match route {
            Route::Nothing | Route::Again => {}
            Route::FreshLink { id, url } => self.take_fresh_link(id, url, cx),
            Route::AskLinks(request) => {
                self.handed_open = true;
                crate::links::open(me, request, cx);
            }
            Route::AddLinks(items) => {
                let added = self.session.add(&self.backend, items);
                if let Some((_, e)) = added.partly() {
                    self.say(e.to_string());
                }
                self.read();
            }
            Route::Listed { url, id, how } => match how {
                // Answered at once where the person ticked "Remember my
                // choice".
                Some(how) => self.answer_duplicate(url, id, how, cx),
                None => self.ask_duplicate(url, id, cx),
            },
            Route::Added(Ok(_)) => {
                self.read();
            }
            Route::Added(Err(e)) => self.say(e),
            Route::Ask(request) => {
                self.handed_open = true;
                crate::details::open_handed(me, request, cx);
            }
        }
    }

    /// The download caught in the browser becomes row `id`'s link, and the
    /// row goes on from where it stopped.
    /// Off the window's thread, as Update download link's OK is: a row still running is stopped
    /// first and that is waited for, up to five seconds (gaps row S17).
    fn take_fresh_link(&mut self, id: dm_core::Id, url: String, cx: &mut Context<Self>) {
        self.session.hands.stop_awaiting_link();
        self.later(
            move |backend| dm_desktop_base::row_edits::update_link(backend, id, &url),
            |view, taken, _| {
                match taken {
                    Ok(()) => view.say(
                        t("The new link was taken from the browser; the download goes on.").into(),
                    ),
                    Err(e) => view.say(e),
                }
                view.read();
            },
            cx,
        );
    }

    /// When done's countdown: shown while it runs, its action carried out
    /// once its time is up (`Session::countdown_due`).
    fn countdown_tick(&mut self, cx: &mut Context<Self>) {
        self.show_countdown(cx);
        self.countdown_now(cx);
    }

    /// The countdown's action, where its time is up.
    pub(crate) fn countdown_now(&mut self, cx: &mut Context<Self>) {
        match self.session.countdown_due(&self.backend, Instant::now()) {
            Some(Ok(dm_shell::app::session::Carried::Quit)) => self.quit(cx),
            Some(Err(e)) => {
                // In the list of problems as well as the status line:
                // whoever set it is likely away, and a status line is
                // overwritten.
                self.problems.push(e.clone());
                self.say(e);
            }
            _ => {}
        }
    }

    /// "Delete file after": the files due go to the Trash, their rows with
    /// them (`dm_shell::app::session::delete_due`).
    fn delete_due_files(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let Some(done) = dm_shell::app::session::delete_due(&self.backend, &self.rows, now) else {
            return;
        };
        for id in &done.gone {
            self.selected.remove(id);
        }
        if !done.failed.is_empty() {
            self.say(tf("Could not delete {}", &[&done.failed.join("; ")]));
        }
        self.read();
    }

    /// The watched folder (Options › Downloads): each file's links added as
    /// the window's own adds go in (`Session::look_at_watched`).
    fn take_watched(&mut self, cx: &mut Context<Self>) {
        let folder = self.backend.settings().watch_folder.clone();
        for links in self
            .session
            .look_at_watched(&folder, &self.rows, &self.backend)
        {
            let _ = self.add(links, Default::default(), cx);
        }
    }
}

/// What hanging up came to, in words: a sentence for the status line, a problem, or nothing
/// for a connection that was not up.
fn hung_up_words(hung: dm_shell::dialup::HungUp) -> Option<Result<String, String>> {
    use dm_shell::dialup::HungUp;
    match hung {
        HungUp::Done(name) => Some(Ok(tf(
            "The scheduled downloads are done: {} is hung up.",
            &[&name],
        ))),
        HungUp::WasDown(_) => None,
        HungUp::NoneSet => Some(Ok(t(
            "Hang up modem when done: no connection is set in Options › Advanced › Dial Up / VPN.",
        )
        .into())),
        HungUp::Failed(name, why) => Some(Err(tf("{} was not hung up: {}", &[&name, &why]))),
    }
}

#[cfg(test)]
mod hang_up_tests {
    use super::hung_up_words;
    use dm_shell::dialup::HungUp;

    /// Scheduler › Hang up modem when done is said as the egui window said it: hung up, no
    /// connection set, or a problem; a connection that was down is not said.
    #[test]
    fn what_hanging_up_came_to_is_said() {
        let done = hung_up_words(HungUp::Done("Office VPN".into()));
        assert!(
            matches!(done, Some(Ok(ref s)) if s.contains("Office VPN") && s.contains("hung up"))
        );
        assert_eq!(hung_up_words(HungUp::WasDown("Office VPN".into())), None);
        assert!(
            matches!(hung_up_words(HungUp::NoneSet), Some(Ok(ref s)) if s.contains("Dial Up / VPN"))
        );
        let failed = hung_up_words(HungUp::Failed("Office VPN".into(), "busy".into()));
        assert!(matches!(failed, Some(Err(ref s)) if s.contains("busy")));
    }
}
