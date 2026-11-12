// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The questions the main window asks: cancel a download, an address
//! already in the list, the ones the engine raises (a name the server sent,
//! a web page for a file), quit while downloads run; the queue they wait
//! in, opened one at a time, when each system lets one open (`sheet`,
//! `show_sheets`); and what each answer does.

use super::*;

impl Downloads {
    /// "Cancel this download?", asked before a download is cancelled: what
    /// was fetched is discarded.
    pub fn ask_cancel(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        use dm_desktop_base::i18n::t;
        let Some(row) = self.rows.iter().find(|r| {
            r.id == id && matches!(r.state, State::Running | State::Queued | State::Paused)
        }) else {
            return;
        };
        let fetched = row.progress.downloaded > 0;
        let settings = self.backend.settings();
        let named = dm_shell::app::names::name_it_will_get_after(
            &self.rows,
            row,
            &settings.download_dir,
            &mut settings.categories(),
            None,
        );
        let asked = crate::question::Asked {
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
        };
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, _, cx| {
            if button != 1 {
                return;
            }
            let _ = view.update(cx, |view, cx| {
                view.backend.cancel(id);
                view.close_progress(id, cx);
                view.progress_closed.insert(id);
                view.read();
                cx.notify();
            });
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// A browser's hand-off has been answered: the next may be asked.
    pub fn handed_closed(&mut self) {
        self.handed_open = false;
    }

    /// Where Add URL's OK sends the address (`add_url::decide`), and the
    /// Recent list it remembers.
    pub fn after_add_url(
        &mut self,
        decided: dm_desktop_base::add_url::Decided,
        cx: &mut Context<Self>,
    ) {
        use dm_desktop_base::add_url::Next;
        if let Some(recent) = decided.remember {
            if let Err(e) = self.backend.remember_urls(recent) {
                self.say(e);
            }
        }
        let me = cx.entity().downgrade();
        match decided.next {
            Next::Says(_) => {}
            Next::Details(addresses) => crate::details::open(me, addresses, cx),
            Next::Duplicate { address, id } => self.ask_duplicate(address, id, cx),
            Next::Answered { address, id, how } => self.answer_duplicate(address, id, how, cx),
            // A playlist is its videos, each a download: listed to be
            // ticked.
            Next::Playlist(address) => crate::playlist::open(me, address, None, cx),
        }
        cx.notify();
    }

    /// "Already in the list": show it (or go on with it), download it
    /// again, replace the old file, or leave it.
    pub(crate) fn ask_duplicate(
        &mut self,
        address: String,
        id: dm_core::Id,
        cx: &mut Context<Self>,
    ) {
        use dm_desktop_base::i18n::{t, tf};
        use dm_shell::app::rules::Dup;
        let status = self
            .rows
            .iter()
            .find(|r| r.id == id)
            .map(|r| dm_desktop_base::rows::status_of(r))
            .unwrap_or_default();
        let (go_on, can_replace) = dm_desktop_base::add_url::duplicate_offers(&self.rows, id);
        // The first, which changes nothing, is Return's: an Return pressed
        // for Download details, a moment before this asked, fetched the file
        // a second time.
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
        let asked = crate::question::Asked {
            title: t("Already in the list").into(),
            lines: vec![
                dm_desktop_base::format::shown_url(&address).into(),
                tf("This address is already in the list ({}).", &[&status]).into(),
            ],
            named: None,
            detail: None,
            toggle: Some(t("Remember my choice").into()),
            ticked: false,
            buttons: choices.iter().map(|(w, _)| (*w).into()).collect(),
            default: 0,
            escape: choices.len() - 1,
        };
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, remember, cx| {
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
        id: dm_core::Id,
        how: dm_shell::app::rules::Dup,
        cx: &mut Context<Self>,
    ) {
        use dm_desktop_base::add_url::Carried;
        use dm_shell::app::rules::GoOn;
        match dm_desktop_base::add_url::carry(&self.rows, address, id, how) {
            Carried::Show { id, go_on } => {
                self.selected.clear();
                self.selected.insert(id);
                match go_on {
                    Some(GoOn::Retry) => self.backend.retry(id),
                    Some(GoOn::Resume) => self.backend.resume(id),
                    _ => {}
                }
                self.read();
            }
            Carried::Details(address) => {
                crate::details::open(cx.entity().downgrade(), vec![address], cx)
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
        self.asks.watch(&self.rows);
        let rows = &self.rows;
        let due = self
            .renames
            .due(|id| rows.iter().find(|r| r.id == id).map(|r| r.state.clone()));
        for (id, name) in due {
            let _ = self.backend.rename(id, &name);
        }
        self.promote_asks(cx);
    }

    /// Asks the next waiting question, as a sheet, when no other is being
    /// asked: none for a row that is gone.
    pub(super) fn promote_asks(&mut self, cx: &mut Context<Self>) {
        use dm_desktop_base::asks;
        use dm_shell::app::questions::Question;
        if self.asking {
            return;
        }
        let rows = &self.rows;
        let Some(question) = self.asks.next(|id| rows.iter().any(|r| r.id == id)) else {
            return;
        };
        let ask = match &question {
            Question::Name {
                address_name,
                server_name,
                ..
            } => asks::name_sent(address_name, server_name),
            Question::Page(_) => asks::page_for_file(),
            Question::NotFound(_) => asks::not_found(),
        };
        let asked = crate::question::Asked {
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
        };
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, _, cx| {
            let _ = view.update(cx, |view, cx| {
                view.asking = false;
                view.answered(question, button == 0);
                view.read();
                view.promote_asks(cx);
                cx.notify();
            });
        });
        self.sheet(asked, None, None, answer, cx);
        self.asking = true;
    }

    /// Carries out the answer to a question a download raised: `yes` the
    /// default (Yes, or OK).
    pub(super) fn answered(&mut self, question: dm_shell::app::questions::Question, yes: bool) {
        use dm_shell::app::questions::Question;
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
                if let Some(now) = self.renames.ask(id, state.as_ref(), &address_name) {
                    let _ = self.backend.rename(id, &now);
                }
            }
            Question::Page(id) if yes => self.backend.accept_page(id),
            Question::Page(id) => self.backend.remove(id),
            _ => {}
        }
    }

    /// Exit, as the menu, ⌘Q or the toolbar ask for it: at once with nothing
    /// downloading, else asked first ("Exit dm?"): exiting pauses the
    /// downloads, and they go on the next time dm opens. A signal (a logout,
    /// `kill`) ends dm without asking, as before.
    pub fn ask_quit(&mut self, cx: &mut Context<Self>) {
        let (busy, waiting) = self.running_and_waiting();
        if busy + waiting == 0 {
            self.quit(cx);
            return;
        }
        if self.exit_asked {
            return;
        }
        self.exit_asked = true;
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, _, cx| {
            let _ = view.update(cx, |view, cx| {
                view.exit_asked = false;
                if button == EXIT {
                    view.quit(cx);
                }
            });
        });
        let live: crate::question::Live = Box::new(|view| Some(exit_question(view)));
        self.sheet(exit_question(self), None, Some(live), answer, cx);
    }

    /// How many downloads are running, and how many are in line to start.
    pub fn running_and_waiting(&self) -> (usize, usize) {
        let count = |state: State| self.rows.iter().filter(|r| r.state == state).count();
        (count(State::Running), count(State::Queued))
    }

    /// Ends dm as Exit does: the queue written down, then every dialog
    /// closed (macOS holds its terminate while a sheet is still asking: a
    /// logout or `kill` waited on an unanswered question), then the
    /// application.
    pub fn quit(&mut self, cx: &mut Context<Self>) {
        // Asked as a question's sheet slides (Exit pressed as "Exit dm?"
        // came in): once it has, as AppKit drops a quit asked inside the
        // slide, and dm ran on.
        #[cfg(target_os = "macos")]
        if crate::sheets::sliding() {
            cx.spawn(async move |this, cx| {
                while crate::sheets::sliding() {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(10))
                        .await;
                }
                let _ = this.update(cx, |this, cx| this.quit(cx));
            })
            .detach();
            return;
        }
        // The downloads stopped and awaited, as a pause stops them, the
        // queue written last (gaps row 186).
        self.backend.shut_down();
        // As the egui window ends: a request still waiting is told dm is
        // quitting, and `control.port` no longer sends a client to a port
        // nobody answers. Here, before the system ends the process:
        // nothing after GPUI's run is reached on macOS.
        dm_desktop_base::control::closing();
        dm_desktop_base::control::forget_port(&dm_shell::settings::home());
        // Where and as large as the window was, for the next start.
        crate::frame::keep(&dm_shell::settings::home(), self.frame, self.display);
        // The questions it closes are not answered, and none waiting is asked.
        crate::question::quitting();
        self.sheets.clear();
        #[cfg(target_os = "macos")]
        crate::sheets::end_all_now();
        for (handle, _, dialog) in crate::screen::Screens::all(cx) {
            if dialog {
                let _ = handle.update(cx, |_, window, _| {
                    // A question's sheet taken off its window first (`sheets`):
                    // macOS holds its quitting while one is up.
                    #[cfg(target_os = "macos")]
                    if let Some(sheet) = crate::syswindow::ns_window(window) {
                        crate::sheets::end_now(&sheet);
                    }
                    window.remove_window()
                });
            }
        }
        cx.quit();
    }

    /// Asks `asked` as a sheet on the main window, after any question
    /// already asked. macOS opens a sheet on whichever window is main: with
    /// Add URL in front the question went with Add URL as it closed, and
    /// with dm in the background it was never shown, so a question waits
    /// until the main window is in front.
    pub fn sheet(
        &mut self,
        asked: crate::question::Asked,
        reword: Option<crate::question::Reword>,
        live: Option<crate::question::Live>,
        answer: crate::question::Answer,
        cx: &mut Context<Self>,
    ) {
        // Asked over the window: one put away in the tray comes back for it.
        if self.hidden {
            self.show_main(cx);
        }
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, ticked, cx| {
            answer(button, ticked, cx);
            let _ = view.update(cx, |view, cx| {
                view.sheet_open = false;
                view.question_window = None;
                view.show_sheets(cx);
                // The hold over the window goes with the question.
                cx.notify();
            });
        });
        self.sheets.push_back((asked, reword, live, answer));
        self.show_sheets(cx);
    }

    /// Opens the next waiting question, when none is open: on macOS and
    /// Windows once the main window is in front (the system gives it the keys
    /// again as a dialog closes, and a question there is a `Dialog`, owned by
    /// the window in front, which must be the main one); on Linux at once,
    /// as the egui window asks there. Waiting for the main window there left
    /// "Already in the list" unseen until it was clicked (B17): the window
    /// manager hands the keys to whichever window it likes as a dialog
    /// closes (openbox: a Download complete window left open), or to none.
    pub(super) fn show_sheets(&mut self, cx: &mut Context<Self>) {
        let main = self.main;
        let in_front = |cx: &Context<Self>| {
            !cfg!(any(target_os = "macos", windows)) || cx.active_window() == Some(main)
        };
        while !self.sheet_open && in_front(cx) {
            let Some((asked, reword, live, answer)) = self.sheets.pop_front() else {
                return;
            };
            let live = live.map(|live| (live, cx.entity()));
            let opened = crate::question::ask(asked, reword, live, answer, cx);
            self.sheet_open = opened.is_some();
            self.question_window = opened;
        }
    }
}
