// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the GPUI window does on each wake beside reading the list
//! (`docs/architecture/ARCHITECTURE.md`): the application's tick
//! (`dm_desktop_base::session`: orders left by the command line, what the
//! read detected, the schedules' hooks and When done) carried out as the
//! window does it, then the browser's hand-offs, the When done countdown,
//! "Delete file after", the watched folder and the one-time hints. Each rule
//! is the application's (`dm_shell::app`); the window says what came of it
//! and opens what it asks in.

use std::time::Instant;

use dm_desktop_base::i18n::{t, tf};
use dm_desktop_base::session::{Driver, Effect};
use gpui::Context;

use crate::window::list::Downloads;

impl Downloads {
    /// One wake's work after the read (`Downloads::read`), on the main
    /// window's thread.
    pub(crate) fn tick(&mut self, cx: &mut Context<Self>) {
        // At rest, the heap's freed pages back to the system (gaps row 175).
        if !self
            .rows
            .iter()
            .any(|r| matches!(r.state, dm_core::State::Running))
        {
            dm_core::heap::give_back();
        }
        // The downloads' speed together, for View › Statistics's average and best (gaps row
        // W15): held in memory, never written.
        let speed: f64 = self
            .rows
            .iter()
            .filter(|r| matches!(r.state, dm_core::State::Running))
            .map(|r| r.progress.speed)
            .sum();
        self.status.speeds.sample(Instant::now(), speed);
        // Windows' High Contrast turned on, off or to another theme: every
        // window drawn again in its colours (`theme.rs`), not only the one in
        // front when it next draws.
        let contrast = dm_desktop_base::contrast::system();
        if contrast != self.layout.contrast {
            self.layout.contrast = contrast;
            cx.refresh_windows();
        }
        // `--new-download` and `--when-done`, left in the inbox by this
        // launch or a later one. In the tests, only by a window whose test
        // holds the inbox, the home being every test's (`harness::Inbox`).
        let driver = Driver::window(crate::reads_inbox());
        for effect in self.session.take_orders(driver) {
            self.carry(effect, cx);
        }
        // What the reads detected, the schedules, each download's own When
        // done and the queue run dry (`Session::tick`), as the window does it.
        for effect in self.session.tick(&self.backend, &self.rows, driver) {
            if matches!(effect, Effect::Quit) {
                self.quit(cx);
                return;
            }
            self.carry(effect, cx);
        }
        // What a platform held back while the person was busy (a
        // fullscreen app) is said once they are not.
        self.backend.platform.tick();
        self.big_download_hint();
        self.details_hint();
        self.take_handed(cx);
        self.countdown_tick(cx);
        self.delete_due_files();
        self.take_watched(cx);
        self.say_rechecked();
        self.take_searches(cx);
        self.take_follows(cx);
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
        self.status.problems.extend(
            problems
                .iter()
                .map(|p| dm_desktop_base::format::worded_problem(p)),
        );
    }

    /// One thing the application's tick asks (`Session::tick`), carried out as the window
    /// does it: a dialog opened, a sentence said in the language in force, a notification, a
    /// sound.
    fn carry(&mut self, effect: Effect, cx: &mut Context<Self>) {
        if let Some(done) = effect.on_platform(&self.backend) {
            if let Err(why) = done {
                self.status.problems.push(why);
            }
            return;
        }
        match effect {
            Effect::NewDownload => crate::dialogs::addurl::open(cx.entity().downgrade(), cx),
            Effect::WhenDone(wanted) => {
                self.run(dm_shell::app::commands::when_done_command(wanted), cx);
            }
            Effect::CannotWhenDone => {
                self.say(
                    t("This computer has no way to do that when the downloads are done.").into(),
                );
            }
            Effect::Telling { expired } => {
                self.prune_progress(cx);
                self.say_opener();
                self.offer_new_link(expired, cx);
            }
            // Told as a finish is, with why in plain words, and a Retry.
            Effect::Failed { id, name, why } => self.backend.platform.notify_failure(
                t("Download failed"),
                &format!("{name} — {why}"),
                id,
            ),
            // A download that started without the window starting it (a
            // browser's hand-off, a second launch) opens its progress window
            // as one started here does, where that is asked for.
            Effect::StartedElsewhere(only) => {
                if !self.progress_closed_has(only) {
                    self.show_progress(only, cx);
                }
            }
            // A browser's hand-off of an address already listed: added by
            // the inbox before the window heard of it, and the browser has
            // let its own download go. Said, as Add URL would have asked.
            Effect::AddedAgain { name } => self.say(tf(
                "Added {} again: that address was already in the list.",
                &[&name],
            )),
            Effect::Finished(ids) => self.finished(&ids, cx),
            // Said aloud from the status bar, not by the system's notification
            // alone, which a screen reader may not read (gaps row U23).
            Effect::Told { finished, failures } => {
                if let Some(sentence) = crate::window::a11y::heard(&finished, &failures) {
                    self.status.heard = Some((sentence, Instant::now()));
                }
            }
            Effect::Problem(why) => self.status.problems.push(why),
            Effect::Armed(wanted) => self.say(tf(
                "The schedule has set what dm does when the downloads are done: {}.",
                &[&t(wanted.verb())],
            )),
            Effect::Scheduled { hang_up } => self.hang_up(hang_up),
            Effect::Quit => self.quit(cx),
            Effect::AskMetered => self.ask_metered(cx),
            // Carried on the platform, above.
            Effect::Notify { .. }
            | Effect::NotifyFile { .. }
            | Effect::Play(_)
            | Effect::Open(_)
            | Effect::Reveal(_) => {}
        }
    }

    /// The connection has become metered and Options › Downloads says to ask (gaps row W11):
    /// every download waits meanwhile; Download anyway lets them go on until the connection is
    /// no longer metered, Wait (the default, which costs nothing) keeps them waiting until then.
    fn ask_metered(&mut self, cx: &mut Context<Self>) {
        let asked = crate::dialogs::question::Asked {
            title: t("The connection is metered").into(),
            lines: vec![t("Downloading over it may cost money or use up a data plan. Every download waits until you choose.").into()],
            named: None,
            detail: None,
            toggle: None,
            ticked: false,
            buttons: vec![t("Wait").into(), t("Download anyway").into()],
            default: 0,
            escape: 0,
        };
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, _, cx| {
            let _ = view.update(cx, |view, cx| {
                view.session.answer_metered(button == 1);
                cx.notify();
            });
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// Scheduler › Hang up modem when done, where the schedules asked for it (`hang_up`): off
    /// the window's thread, as Windows takes a moment to end a connection; what it came to is
    /// said when it is known.
    fn hang_up(&mut self, hang_up: bool) {
        if hang_up && cfg!(windows) {
            if let Some(hung) =
                dm_shell::dialup::hang_up_later(std::sync::Arc::clone(&self.threads.hung_up))
            {
                self.say_hung_up(hung);
            }
        }
        let hung = dm_core::lock(&self.threads.hung_up).take();
        if let Some(hung) = hung {
            self.say_hung_up(hung);
        }
    }

    /// Says what hanging up came to: a connection that was not up needs no word, one
    /// Windows would not end is a problem.
    fn say_hung_up(&mut self, hung: dm_shell::dialup::HungUp) {
        match hung_up_words(hung) {
            Some(Ok(sentence)) => self.say(sentence),
            Some(Err(problem)) => self.status.problems.push(problem),
            None => {}
        }
    }

    /// A very big download that has started: said once, ever, that the
    /// Scheduler can run it at night.
    pub(crate) fn big_download_hint(&mut self) {
        use dm_shell::app::hints::Hint;
        let settings = self.backend.settings();
        if !Hint::BigDownload.due(&settings, &self.rows, self.status.said.is_none()) {
            return;
        }
        self.say(t("This is a big download. Downloads › Scheduler can run it at night, when you are not using the computer.").into());
        let mut wanted = settings;
        Hint::BigDownload.mark(&mut wanted);
        if let Err(e) = self.backend.set_settings(wanted) {
            self.say(e);
        }
    }

    /// A download under way while its progress window does not open by itself: said once,
    /// ever, where its progress is (the row's Details button),
    /// and only where nothing else is being said.
    pub(crate) fn details_hint(&mut self) {
        use dm_shell::app::hints::Hint;
        let settings = self.backend.settings();
        if !Hint::Details.due(&settings, &self.rows, self.status.said.is_none()) {
            return;
        }
        self.say(t("Hover a download and press Details to watch its progress.").into());
        let mut wanted = settings;
        Hint::Details.mark(&mut wanted);
        if let Err(e) = self.backend.set_settings(wanted) {
            self.say(e);
        }
    }

    /// A download handed over by the browser, routed by the application
    /// (`Hands::next`): Download details for it, the question for an
    /// address already listed, or added straight in.
    fn take_handed(&mut self, cx: &mut Context<Self>) {
        use dm_shell::app::handoff::{Asking, Route};
        let asking = if self.questions.handed_open {
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
                self.questions.handed_open = true;
                crate::dialogs::links::open(me, request, cx);
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
                self.questions.handed_open = true;
                crate::dialogs::details::open_handed(me, request, cx);
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
                self.status.problems.push(e.clone());
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
            self.list.selected.remove(id);
        }
        if !done.failed.is_empty() {
            self.say(tf("Could not delete {}", &[&done.failed.join("; ")]));
        }
        self.read();
    }

    /// The watched folder (Options › Downloads): each file's links added as
    /// the window's own adds go in (`Session::look_at_watched`).
    fn take_watched(&mut self, cx: &mut Context<Self>) {
        let folder = self.backend.settings().downloads.watch_folder.clone();
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

    /// What Scheduler › Hang up modem when done came to is said: hung up, no
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
