// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The questions the main window asks: cancel a download, an address
//! already in the list, the ones the engine raises (a name the server sent,
//! a web page for a file), quit while downloads run; the queue they wait
//! in, opened one at a time, when each system lets one open (`sheet`,
//! `show_sheets`); and what each answer does.

use super::*;

/// How often a quit asked while a question's sheet slides looks at whether it has stopped.
#[cfg(target_os = "macos")]
const SLIDING_LOOK: std::time::Duration = std::time::Duration::from_millis(10);

/// When done's countdown as it stands: what EDM will do and in how many
/// seconds; `None` once it is over or cancelled.
pub(super) fn countdown_question(view: &Downloads) -> Option<crate::dialogs::question::Asked> {
    use edm_desktop_base::i18n::{t, tf};
    let (action, at) = view.session.countdown?;
    let left = at
        .saturating_duration_since(std::time::Instant::now())
        .as_secs();
    Some(crate::dialogs::question::Asked {
        title: t("Downloads are done").into(),
        lines: vec![tf("EDM will {} in {} seconds.", &[&t(action.verb()), &left]).into()],
        named: None,
        detail: None,
        toggle: None,
        ticked: false,
        // Cancel, the default, rightmost: Return and Escape both stop it.
        buttons: vec![t("Cancel").into(), t("Now").into()],
        default: 0,
        escape: 0,
        destroys: None,
    })
}

/// Where Exit stands in "Exit EDM?"'s buttons.
pub(super) const EXIT: usize = 1;

/// "Exit EDM?" as the list stands (`asks::exit`). Keep downloading (Stay,
/// once nothing runs) stands rightmost and Exit beside it, each where it is
/// whatever the words say: a download finishing while it is open changes
/// the words and moves the default to Exit, and moves no button.
pub(super) fn exit_question(view: &Downloads) -> crate::dialogs::question::Asked {
    let (busy, waiting) = view.running_and_waiting();
    let ask = edm_desktop_base::asks::exit(busy, waiting);
    crate::dialogs::question::Asked {
        title: ask.title.into(),
        lines: vec![ask.sentence.into()],
        named: None,
        detail: None,
        toggle: None,
        ticked: false,
        buttons: vec![ask.keep.into(), ask.exit.into()],
        default: if ask.finished { EXIT } else { 0 },
        escape: 0,
        destroys: None,
    }
}

impl Downloads {
    /// "Cancel this download?", asked before a download is cancelled: what
    /// was fetched is discarded.
    pub fn ask_cancel(&mut self, id: edm_core::Id, cx: &mut Context<Self>) {
        use edm_desktop_base::i18n::t;
        let Some(row) = self.rows.iter().find(|r| {
            r.id == id && matches!(r.state, State::Running | State::Queued | State::Paused)
        }) else {
            return;
        };
        let fetched = row.progress.downloaded > 0;
        let settings = self.backend.settings();
        let named = edm_shell::app::names::name_it_will_get_after(
            &self.rows,
            row,
            &settings.folders.download_dir,
            &mut settings.categories(),
            None,
        );
        let asked = crate::dialogs::question::Asked {
            title: t("Cancel this download?").into(),
            lines: fetched
                .then(|| t("Cancelling discards what was downloaded.").into())
                .into_iter()
                .collect(),
            named: Some(named.into()),
            detail: None,
            toggle: None,
            ticked: false,
            buttons: vec![t("Keep downloading").into(), t("Yes, cancel").into()],
            default: 0,
            escape: 0,
            destroys: None,
        };
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, _, cx| {
            if button != 1 {
                return;
            }
            let _ = view.update(cx, |view, cx| {
                // Off the window's thread: its staged parts are thrown away,
                // many files for a big download. Its progress window closes
                // as the row reads cancelled.
                view.later(
                    move |backend| backend.cancel(id),
                    move |view, (), cx| {
                        view.close_progress(id, cx);
                        view.windows.progress_closed.insert(id);
                        view.read();
                    },
                    cx,
                );
                cx.notify();
            });
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// A browser's hand-off has been answered: the next may be asked.
    pub fn handed_closed(&mut self) {
        self.questions.handed_open = false;
    }

    /// Where Add URL's OK sends the address (`add_url::decide`), and the
    /// Recent list it remembers.
    pub fn after_add_url(
        &mut self,
        decided: edm_desktop_base::add_url::Decided,
        cx: &mut Context<Self>,
    ) {
        use edm_desktop_base::add_url::Next;
        if let Some(recent) = decided.remember {
            if let Err(e) = self.backend.remember_urls(recent) {
                self.say(e);
            }
        }
        let me = cx.entity().downgrade();
        match decided.next {
            Next::Says(_) => {}
            Next::Details(addresses) => crate::dialogs::details::open(me, addresses, cx),
            Next::Duplicate { address, id } => self.ask_duplicate(address, id, cx),
            Next::Answered { address, id, how } => self.answer_duplicate(address, id, how, cx),
            // A playlist is its videos, each a download: listed to be
            // ticked.
            Next::Playlist(address) => crate::dialogs::playlist::open(me, address, None, cx),
        }
        cx.notify();
    }

    /// "Already in the list": show it (or go on with it), download it
    /// again, replace the old file, or leave it.
    pub(crate) fn ask_duplicate(
        &mut self,
        address: String,
        id: edm_core::Id,
        cx: &mut Context<Self>,
    ) {
        use edm_desktop_base::i18n::{t, tf};
        use edm_shell::app::rules::Dup;
        let status = self
            .rows
            .iter()
            .find(|r| r.id == id)
            .map(|r| edm_desktop_base::rows::status_of(r))
            .unwrap_or_default();
        let (go_on, can_replace) = edm_desktop_base::add_url::duplicate_offers(&self.rows, id);
        // The first, which changes nothing, is Return's: a Return pressed
        // for Download details, a moment before this asks, would otherwise
        // fetch the file a second time.
        let mut choices = vec![
            (
                if go_on { t("Resume it") } else { t("Show it") },
                Some(Dup::Show),
            ),
            (t("Download again"), Some(Dup::Again)),
        ];
        if can_replace {
            choices.push((t("Replace the old file"), Some(Dup::Replace)));
        }
        choices.push((t("Cancel"), None));
        let asked = crate::dialogs::question::Asked {
            title: t("Already in the list").into(),
            lines: vec![
                edm_desktop_base::format::shown_url(&address).into(),
                tf("This address is already in the list ({}).", &[&status]).into(),
            ],
            named: None,
            detail: None,
            toggle: Some(t("Remember my choice").into()),
            ticked: false,
            buttons: choices.iter().map(|(w, _)| (*w).into()).collect(),
            default: 0,
            escape: choices.len() - 1,
            destroys: None,
        };
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, remember, cx| {
            let Some(how) = choices.get(button).and_then(|(_, how)| *how) else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                if remember {
                    let mut wanted = view.backend.settings();
                    wanted.duplicate_choice = how.word().to_string();
                    if let Err(e) = view.backend.set_settings(wanted) {
                        view.say(e);
                    }
                }
                view.answer_duplicate(address, id, how, cx);
            });
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// Carries out an answer to "Already in the list", asked or remembered
    /// (`add_url::carry`).
    pub(crate) fn answer_duplicate(
        &mut self,
        address: String,
        id: edm_core::Id,
        how: edm_shell::app::rules::Dup,
        cx: &mut Context<Self>,
    ) {
        use edm_desktop_base::add_url::Carried;
        use edm_shell::app::rules::GoOn;
        match edm_desktop_base::add_url::carry(&self.rows, address, id, how) {
            Carried::Show { id, go_on } => {
                self.list.selected.clear();
                self.list.selected.insert(id);
                // Off the window's thread, as the toolbar's Resume is.
                match go_on {
                    Some(GoOn::Retry) => self.later_then_read(move |backend| backend.retry(id), cx),
                    Some(GoOn::Resume) => {
                        self.later_then_read(move |backend| backend.resume(id), cx)
                    }
                    _ => {}
                }
                self.read();
            }
            Carried::Details(address) => {
                crate::dialogs::details::open(cx.entity().downgrade(), vec![address], cx)
            }
            Carried::Replace { address, with } => {
                if let Err(e) = self.add(vec![address], *with, cx) {
                    self.say(e);
                }
            }
        }
        cx.notify();
    }

    /// Looks at the rows as just read for the questions they raise, gives
    /// the names chosen for rows that have now finished, and asks the next
    /// question when none is being asked.
    pub(super) fn watch_asks(&mut self, cx: &mut Context<Self>) {
        self.questions.asks.watch(&self.rows);
        let rows = &self.rows;
        let due = self
            .questions
            .renames
            .due(|id| rows.iter().find(|r| r.id == id).map(|r| r.state.clone()));
        // Off the window's thread: a file renamed on its disk.
        if !due.is_empty() {
            self.later_then_read(
                move |backend| {
                    for (id, name) in &due {
                        let _ = backend.rename(*id, name);
                    }
                },
                cx,
            );
        }
        self.promote_asks(cx);
    }

    /// Asks the next waiting question, as a sheet, when no other is being
    /// asked: none for a row that is gone.
    pub(super) fn promote_asks(&mut self, cx: &mut Context<Self>) {
        use edm_desktop_base::asks;
        use edm_shell::app::questions::Question;
        if self.questions.asking {
            return;
        }
        let rows = &self.rows;
        let Some(question) = self
            .questions
            .asks
            .next(|id| rows.iter().any(|r| r.id == id))
        else {
            return;
        };
        // On a phone a download shared from another app starts as it is, under the name its
        // server sends (a video's title, not its address's last part): the question's own
        // default, taken without stopping it to ask.
        if crate::platform::ON_A_PHONE && matches!(question, Question::Name { .. }) {
            return self.promote_asks(cx);
        }
        let ask = match &question {
            Question::Name {
                address_name,
                server_name,
                ..
            } => asks::name_sent(address_name, server_name),
            Question::Page(_) => asks::page_for_file(),
            // With "Remember my choice", as "Already in the list" is (gaps row W18).
            Question::SameFile(id) => return self.ask_same_file(*id, cx),
        };
        let asked = crate::dialogs::question::Asked {
            title: ask.title.into(),
            lines: ask.lines.into_iter().map(Into::into).collect(),
            named: None,
            detail: ask.detail.map(Into::into),
            toggle: None,
            ticked: false,
            buttons: ask.buttons.into_iter().map(Into::into).collect(),
            // Escape keeps the default, which changes nothing.
            default: 0,
            escape: 0,
            destroys: None,
        };
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, _, cx| {
            let _ = view.update(cx, |view, cx| {
                view.questions.asking = false;
                view.answered(question, button == 0, cx);
                view.read();
                view.promote_asks(cx);
                cx.notify();
            });
        });
        self.sheet(asked, None, None, answer, cx);
        self.questions.asking = true;
    }

    /// Carries out the answer to a question a download raised: `yes` the
    /// default (Yes, or OK). Carried out off the window's thread, and the
    /// list read again once it is: each renames a file or writes the queue.
    pub(super) fn answered(
        &mut self,
        question: edm_shell::app::questions::Question,
        yes: bool,
        cx: &mut Context<Self>,
    ) {
        use edm_shell::app::questions::Question;
        match question {
            // No: the file is given the address's name, at once if it has
            // finished, else as it finishes.
            Question::Name {
                id, address_name, ..
            } if !yes => {
                let state = self
                    .rows
                    .iter()
                    .find(|r| r.id == id)
                    .map(|r| r.state.clone());
                if let Some(now) = self
                    .questions
                    .renames
                    .ask(id, state.as_ref(), &address_name)
                {
                    self.later_then_read(
                        move |backend| {
                            let _ = backend.rename(id, &now);
                        },
                        cx,
                    );
                }
            }
            Question::Page(id) if yes => {
                self.later_then_read(move |backend| backend.accept_page(id), cx)
            }
            Question::Page(id) => self.later_then_read(move |backend| backend.remove(id), cx),
            _ => {}
        }
    }

    /// Exit, as the menu, ⌘Q or the toolbar ask for it: at once with nothing
    /// downloading, else asked first ("Exit EDM?"): exiting pauses the
    /// downloads, and they go on the next time EDM opens. A signal (a logout,
    /// `kill`) ends EDM without asking, as before.
    pub fn ask_quit(&mut self, cx: &mut Context<Self>) {
        let (busy, waiting) = self.running_and_waiting();
        if busy + waiting == 0 {
            self.quit(cx);
            return;
        }
        if self.questions.exit_asked {
            return;
        }
        self.questions.exit_asked = true;
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, _, cx| {
            let _ = view.update(cx, |view, cx| {
                view.questions.exit_asked = false;
                if button == EXIT {
                    view.quit(cx);
                } else {
                    // What finished while it asked, listed now.
                    view.finished(&[], cx);
                }
            });
        });
        let live: crate::dialogs::question::Live = Box::new(|view| Some(exit_question(view)));
        self.sheet(exit_question(self), None, Some(live), answer, cx);
    }

    /// How many downloads are running, and how many are in line to start.
    pub fn running_and_waiting(&self) -> (usize, usize) {
        let count = |state: State| self.rows.iter().filter(|r| r.state == state).count();
        (count(State::Running), count(State::Queued))
    }

    /// Ends EDM as Exit does: the queue written down, then every dialog
    /// closed (macOS holds its terminate while a sheet is still asking: a
    /// logout or `kill` waited on an unanswered question), then the
    /// application.
    pub fn quit(&mut self, cx: &mut Context<Self>) {
        // Asked as a question's sheet slides (Exit pressed as "Exit EDM?"
        // came in): once it has, as AppKit drops a quit asked inside the
        // slide, and EDM ran on.
        #[cfg(target_os = "macos")]
        if crate::platform::sheets::sliding() {
            cx.spawn(async move |this, cx| {
                while crate::platform::sheets::sliding() {
                    cx.background_executor().timer(SLIDING_LOOK).await;
                }
                let _ = this.update(cx, |this, cx| this.quit(cx));
            })
            .detach();
            return;
        }
        self.wind_down();
        // The questions it closes are not answered, and none waiting is asked.
        crate::dialogs::question::quitting();
        self.questions.sheets.clear();
        #[cfg(target_os = "macos")]
        crate::platform::sheets::end_all_now();
        for (handle, _, dialog) in crate::window::screen::Screens::all(cx) {
            if dialog {
                let _ = handle.update(cx, |_, window, _| {
                    // A question's sheet taken off its window first (`sheets`):
                    // macOS holds its quitting while one is up.
                    #[cfg(target_os = "macos")]
                    if let Some(sheet) = crate::platform::syswindow::ns_window(window) {
                        crate::platform::sheets::end_now(&sheet);
                    }
                    window.remove_window()
                });
            }
        }
        self.questions.quit_asked = true;
        cx.quit();
    }

    /// What Exit does before the window goes, and what GPUI's own end of EDM
    /// does where Exit was not chosen (`on_app_quit`: Windows ending the
    /// session, a logoff, a shutdown or restart, quits GPUI from its own
    /// window and ends the process as soon as the quit's handlers return).
    pub(crate) fn wind_down(&mut self) {
        // On a phone the window's awake watch steps aside first: the stop below would have it
        // let go of the foreground service, and Android may end a process without one at once,
        // before a windowless EDM takes the downloads up again (`edm-android`'s engine). Where
        // none does, the process ends, and the service with it.
        if crate::platform::ON_A_PHONE {
            edm_desktop_base::awake::retire_phone_watch();
        }
        // The downloads stopped and awaited, as a pause stops them, the
        // queue written last (gaps row 186).
        self.backend.shut_down();
        // A request still waiting is told EDM is
        // quitting, and `control.port` no longer sends a client to a port
        // nobody answers. Here, before the system ends the process:
        // nothing after GPUI's run is reached on macOS. Not in the window's tests, which
        // share one process: one test's Exit would close every other's port
        // (`edm_desktop_base::control`'s own tests check the closing).
        #[cfg(not(test))]
        edm_desktop_base::control::closing();
        edm_desktop_base::control::forget_port(&edm_shell::settings::home());
        // Where and as large as the window was, for the next start.
        crate::window::frame::keep(
            &edm_shell::settings::home(),
            self.windows.frame,
            self.windows.display,
        );
    }

    /// Asks `asked` as a sheet on the main window, after any question
    /// already asked. macOS opens a sheet on whichever window is main: with
    /// Add URL in front the question went with Add URL as it closed, and
    /// with EDM in the background it was never shown, so a question waits
    /// until the main window is in front.
    pub fn sheet(
        &mut self,
        asked: crate::dialogs::question::Asked,
        reword: Option<crate::dialogs::question::Reword>,
        live: Option<crate::dialogs::question::Live>,
        answer: crate::dialogs::question::Answer,
        cx: &mut Context<Self>,
    ) {
        // Asked over the window: one put away in the tray comes back for it.
        if self.system.hidden {
            self.show_main(cx);
        }
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, ticked, cx| {
            answer(button, ticked, cx);
            let _ = view.update(cx, |view, cx| {
                view.questions.sheet_open = false;
                view.questions.question_window = None;
                view.show_sheets(cx);
                // The hold over the window goes with the question.
                cx.notify();
            });
        });
        self.questions
            .sheets
            .push_back((asked, reword, live, answer));
        // Asked from another of EDM's windows (a progress window's Cancel download): the main
        // window, which a question waits for on macOS and Windows, is brought in front, and its
        // coming in front shows it (`observe_window_activation`). Without it the question
        // would wait unseen until the person clicked the main window (found by the window's
        // tests).
        let main = self.questions.main;
        if cfg!(any(target_os = "macos", windows))
            && !self.questions.sheet_open
            && cx.active_window() != Some(main)
        {
            cx.defer(move |cx| {
                let _ = main.update(cx, |_, window, _| window.activate_window());
            });
        }
        self.show_sheets(cx);
    }

    /// Opens the next waiting question, when none is open: on macOS and
    /// Windows once the main window is in front (the system gives it the keys
    /// again as a dialog closes, and a question there is a `Dialog`, owned by
    /// the window in front, which must be the main one); on Linux at once.
    /// Waiting for the main window there left
    /// "Already in the list" unseen until it was clicked (B17): the window
    /// manager hands the keys to whichever window it likes as a dialog
    /// closes (openbox: a Download complete window left open), or to none.
    pub(super) fn show_sheets(&mut self, cx: &mut Context<Self>) {
        let main = self.questions.main;
        let in_front = |cx: &Context<Self>| {
            !cfg!(any(target_os = "macos", windows)) || cx.active_window() == Some(main)
        };
        while !self.questions.sheet_open && in_front(cx) {
            let Some((asked, reword, live, answer)) = self.questions.sheets.pop_front() else {
                return;
            };
            let live = live.map(|live| (live, cx.entity()));
            let opened = crate::dialogs::question::ask(asked, reword, live, answer, cx);
            self.questions.sheet_open = opened.is_some();
            self.questions.question_window = opened;
        }
    }
}
