// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The download list as the shell draws it: the application read once each
//! time the window is woken (a queue change, coalesced over a quarter of a
//! second as the egui window does; an order or a signal at once; and once a
//! second for the schedules' and When done's clocks), and only the rows on
//! screen laid out (`uniform_list`), so a list of 10,000 costs what the
//! screenful does.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use dm_core::{Entry, State};
use dm_desktop_base::boot::Wake;
use dm_shell::app::backend::Backend;
use dm_shell::app::session::Session;
use gpui::{
    div, prelude::*, Context, FocusHandle, Role, SharedString, UniformListScrollHandle, Window,
};

use crate::screen::Screen;
use crate::theme::Theme;
use dm_desktop_base::columns::Column;
use dm_shell::app::places::{Place, Places};

/// Runs `update` on the view in the window the loop was started in, the
/// main window: not `WeakEntity::update_in`, which runs in whichever window
/// last drew the view (Download details reads it as it draws, and once that
/// dialog closed the view had no window, and the loop ended). `None` once
/// the window or the view is gone.
fn in_own_window<R>(
    this: &gpui::WeakEntity<Downloads>,
    cx: &mut gpui::AsyncWindowContext,
    update: impl FnOnce(&mut Downloads, &mut Window, &mut Context<Downloads>) -> R,
) -> Option<R> {
    cx.update(|window, cx| this.update(cx, |this, cx| update(this, window, cx)).ok())
        .ok()
        .flatten()
}

/// When done's countdown as it stands: what dm will do and in how many
/// seconds; `None` once it is over or cancelled.
fn countdown_question(view: &Downloads) -> Option<crate::question::Asked> {
    use dm_desktop_base::i18n::{t, tf};
    let (action, at) = view.session.countdown?;
    let left = at
        .saturating_duration_since(std::time::Instant::now())
        .as_secs();
    Some(crate::question::Asked {
        title: t("Downloads are done").into(),
        lines: vec![tf("dm will {} in {} seconds.", &[&t(action.verb()), &left]).into()],
        named: None,
        detail: None,
        toggle: None,
        // Cancel, the default, rightmost: Return and Escape both stop it.
        buttons: vec![t("Cancel").into(), t("Now").into()],
        default: 0,
        escape: 0,
    })
}

/// Where Exit stands in "Exit dm?"'s buttons.
const EXIT: usize = 1;

/// "Exit dm?" as the list stands (`asks::exit`). Keep downloading (Stay,
/// once nothing runs) stands rightmost and Exit beside it, each where it is
/// whatever the words say: a download finishing while it is open changes
/// the words and moves the default to Exit, and moves no button.
fn exit_question(view: &Downloads) -> crate::question::Asked {
    let (busy, waiting) = view.running_and_waiting();
    let ask = dm_desktop_base::asks::exit(busy, waiting);
    crate::question::Asked {
        title: ask.title.into(),
        lines: vec![ask.sentence.into()],
        named: None,
        detail: None,
        toggle: None,
        buttons: vec![ask.keep.into(), ask.exit.into()],
        default: if ask.finished { EXIT } else { 0 },
        escape: 0,
    }
}

/// The rows' height, in points.
pub(crate) const ROW_HEIGHT: f32 = 24.0;

/// How often a window with nothing moving wakes of its own accord.
const IDLE: Duration = Duration::from_secs(30);

/// The main window's view: the application, and the rows it last read.
pub struct Downloads {
    pub(crate) backend: Backend,
    pub(crate) session: Session,
    pub(crate) rows: Vec<Arc<Entry>>,
    /// The control port's requests, answered on this thread.
    requests: Option<std::sync::mpsc::Receiver<dm_desktop_base::control::Request>>,
    /// The port was opened only for AI assistants.
    assistants_only: bool,
    /// Where the list is scrolled, and where it is on screen.
    pub(crate) scroll: UniformListScrollHandle,
    /// The words on screen, where the last frame laid them out.
    pub(crate) screen: Screen,
    /// The window's focus: where keys and the menu bar's commands arrive.
    pub(crate) focus: FocusHandle,
    /// The place chosen in the side panel.
    pub(crate) place: Place,
    /// Each row's place and the places' counts (`dm_shell::app::places`).
    pub(crate) places: Places,
    /// The rows the list shows, in the place chosen: their indices in `rows`.
    pub(crate) shown: Vec<usize>,
    /// The column the list is sorted by, and whether up; none: as added.
    pub(crate) sort: Option<(Column, bool)>,
    /// The rows chosen, by id: they stay chosen as the list is read again
    /// and sorted, as a person expects of a selection.
    pub(crate) selected: std::collections::BTreeSet<dm_core::Id>,
    /// Where a ⇧-click's range starts: the row last clicked alone.
    anchor: Option<dm_core::Id>,
    /// The row menu, open where it was asked for.
    menu: Option<gpui::Point<gpui::Pixels>>,
    /// A sentence for the status bar, and when it was said: it stands for
    /// a while, as the egui window's does.
    pub(crate) said: Option<(String, std::time::Instant)>,
    /// Finished rows Remove took off, kept a moment for Undo.
    pub(crate) undo: dm_shell::app::backend::Undo,
    /// The questions the rows raise as they run (B39 to B41), asked one at
    /// a time in the order they came.
    asks: dm_shell::app::questions::Questions,
    /// Names a person chose for rows still downloading, given as each
    /// finishes.
    renames: dm_shell::app::names::PendingRenames,
    /// A question is being asked: the next waits for its answer.
    asking: bool,
    /// The main window: a sheet is opened on it alone.
    main: gpui::AnyWindowHandle,
    /// Questions waiting to be asked as sheets, one at a time.
    sheets: std::collections::VecDeque<(
        crate::question::Asked,
        Option<crate::question::Reword>,
        Option<crate::question::Live>,
        crate::question::Answer,
    )>,
    /// A sheet is open on the main window.
    sheet_open: bool,
    /// "Exit dm?" is being asked: Exit again does not ask twice.
    exit_asked: bool,
    /// What the reads since the last tick detected (`tick.rs`).
    pub(crate) detected: Vec<dm_shell::app::detect::Detected>,
    /// Problems to show above the list, each until dismissed.
    pub(crate) problems: Vec<String>,
    /// Finished rows' sizes, read once from their files.
    saved: dm_shell::app::snapshot::SavedSizes,
    /// A browser's hand-off is being asked about (Download details or
    /// Download links): the next waits for it.
    pub(crate) handed_open: bool,
    /// Hand-offs cancelled, by site (`handoff::Cancels`): the second asks
    /// whether to leave that site's downloads to the browser.
    cancelled_hand_offs: dm_shell::app::handoff::Cancels,
    /// When done's countdown is being shown.
    countdown_shown: bool,
    /// The top bar's search box: the words that narrow the list.
    pub(crate) filter: gpui::Entity<gpui_component::input::InputState>,
    /// Its words, as last typed.
    filter_text: String,
    /// The speed limit before the limiter was turned off, for turning it on.
    last_limit: u64,
    /// The Recheck queue's counts, once a check of it is done.
    rechecked: std::sync::Arc<std::sync::Mutex<Option<(usize, usize)>>>,
    /// The queues the menu bar lists, to build it again when they change.
    menu_queues: Vec<String>,
    /// What saved searches found, as each run ends, by the search's name.
    #[allow(clippy::type_complexity)]
    search_results:
        std::sync::Arc<std::sync::Mutex<Vec<(String, Result<dm_shell::lists::Crawled, String>)>>>,
    /// When the saved searches were last looked at.
    searches_checked: Option<std::time::Instant>,
    /// Each progress window open, by its download.
    progress_windows: std::collections::BTreeMap<dm_core::Id, gpui::AnyWindowHandle>,
    /// Progress windows a person closed: not opened again by themselves.
    progress_closed: std::collections::HashSet<dm_core::Id>,
    /// The downloads "Download complete" lists.
    pub(crate) complete: std::collections::BTreeSet<dm_core::Id>,
    /// "Download complete", while it is open.
    complete_window: Option<gpui::AnyWindowHandle>,
    /// What an opener said went wrong, from its thread.
    opener_said: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// View › Categories: the side panel shown.
    show_sidebar: bool,
    /// View › Toolbar: the toolbar shown.
    show_toolbar: bool,
}

impl Downloads {
    /// The view, reading the queue now and each time `woken` says.
    pub fn new(
        backend: Backend,
        woken: async_channel::Receiver<Wake>,
        requests: Option<std::sync::mpsc::Receiver<dm_desktop_base::control::Request>>,
        assistants_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Downloads {
        let mut this = Downloads {
            backend,
            session: Session::default(),
            rows: Vec::new(),
            requests,
            assistants_only,
            scroll: UniformListScrollHandle::new(),
            screen: Screen::default(),
            focus: cx.focus_handle(),
            place: Place::All,
            places: Places::default(),
            shown: Vec::new(),
            sort: None,
            selected: Default::default(),
            anchor: None,
            menu: None,
            said: None,
            undo: Default::default(),
            asks: Default::default(),
            renames: Default::default(),
            asking: false,
            main: window.window_handle(),
            sheets: Default::default(),
            sheet_open: false,
            exit_asked: false,
            detected: Vec::new(),
            problems: Vec::new(),
            saved: Default::default(),
            handed_open: false,
            cancelled_hand_offs: Default::default(),
            countdown_shown: false,
            filter: cx.new(|cx| {
                gpui_component::input::InputState::new(window, cx)
                    .placeholder(dm_desktop_base::i18n::t("Name, address or note"))
            }),
            filter_text: String::new(),
            last_limit: 0,
            rechecked: Default::default(),
            menu_queues: Vec::new(),
            search_results: Default::default(),
            searches_checked: None,
            progress_windows: Default::default(),
            progress_closed: Default::default(),
            complete: Default::default(),
            complete_window: None,
            opener_said: Default::default(),
            show_sidebar: true,
            show_toolbar: true,
        };
        // The sort the person left the list in.
        let settings = this.backend.settings();
        this.sort = Column::from_key(&settings.sort_column).map(|c| (c, settings.sort_ascending));
        window.focus(&this.focus, cx);
        crate::screen::Screens::register(window.window_handle(), this.screen.clone(), false, cx);
        this.read();
        // One task on the window's thread: each wake is one read, a burst of
        // queue changes one read a quarter of a second later.
        // Redrawn in the system's colours when the person switches between
        // light and dark.
        // The list narrowed as one types in the top bar's search box;
        // Return there chooses the first row it leaves.
        cx.subscribe_in(
            &this.filter,
            window,
            |this: &mut Downloads, field, event, window, cx| match event {
                gpui_component::input::InputEvent::Change => {
                    this.filter_text = field.read(cx).value().to_string();
                    this.reshow();
                    cx.notify();
                }
                gpui_component::input::InputEvent::PressEnter { .. } => {
                    this.find_step(true);
                    window.focus(&this.focus, cx);
                    cx.notify();
                }
                _ => {}
            },
        )
        .detach();
        // A question waiting for the main window is asked once it is in
        // front again.
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.show_sheets(cx);
            }
        })
        .detach();
        cx.observe_window_appearance(window, |_, window, cx| {
            // The text fields and menus (gpui-component's) follow too.
            gpui_component::Theme::sync_system_appearance(Some(window), cx);
            cx.notify();
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| loop {
            // Nothing tells the window of a clock (the schedules, When
            // done's countdown): once a second while a download runs or a
            // countdown is on, else every 30 seconds, as the egui window.
            let every = this
                .read_with(cx, |this, _| this.clock_every())
                .unwrap_or(IDLE);
            let wake = futures_lite::future::or(async { woken.recv().await.ok() }, async {
                cx.background_executor().timer(every).await;
                Some(Wake::Now)
            })
            .await;
            let Some(wake) = wake else {
                return;
            };
            if wake == Wake::Soon {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
            }
            while woken.try_recv().is_ok() {}
            // Asked to end (a logout, `kill`, an uninstall): as Exit does,
            // the queue written down first.
            if dm_desktop_base::signals::SIGNALLED.load(Ordering::SeqCst) {
                let _ = in_own_window(&this, cx, |this, _, cx| this.quit(cx));
                return;
            }
            if in_own_window(&this, cx, |this, window, cx| {
                this.read();
                this.tick(cx);
                this.watch_asks(cx);
                this.answer_requests(window, cx);
                cx.notify();
            })
            .is_none()
            {
                return;
            }
        })
        .detach();
        this
    }

    /// How long the window sleeps when nothing wakes it.
    fn clock_every(&self) -> Duration {
        if self.session.countdown.is_some() || self.rows.iter().any(|r| r.state == State::Running) {
            Duration::from_secs(1)
        } else {
            IDLE
        }
    }

    /// Answers what the control port asked, in order: who may ask as the
    /// application says, the window's methods through its `WindowProbe`,
    /// the rest through the application (`control_api`).
    fn answer_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let asked: Vec<_> = match &self.requests {
            Some(requests) => requests.try_iter().collect(),
            None => return,
        };
        let allow_mcp = self.backend.settings().allow_mcp;
        for request in asked {
            let answer = match dm_shell::app::control_api::refused(
                request.via_mcp,
                self.assistants_only,
                allow_mcp,
            ) {
                Some(why) => Err(why),
                None => crate::probe::answer(self, &request.method, &request.params, window, cx),
            };
            let _ = request.reply.send(answer);
        }
    }

    /// Carries out a command from the menu bar or a key. What the shell
    /// cannot do yet is greyed in the menus (`menus::carried_out`), and a
    /// key for it does nothing.
    pub fn run(&mut self, command: dm_shell::app::commands::Command, cx: &mut Context<Self>) {
        use dm_shell::app::commands::Command;
        if !self.enabled(command) {
            return;
        }
        if command == Command::Quit {
            self.ask_quit(cx);
            return;
        }
        let chosen: Vec<Entry> = self.chosen().into_iter().cloned().collect();
        match command {
            // Each address on a line of its own, without a sign-in that may
            // be in it: what is pasted elsewhere is the link.
            Command::CopyLink => {
                let links: Vec<String> = chosen
                    .iter()
                    .map(|r| dm_desktop_base::format::copyable_url(&r.url))
                    .collect();
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(links.join("\n")));
                self.say(dm_desktop_base::i18n::tn(
                    "Copied {} link.||Copied {} links.",
                    links.len() as u64,
                    &[&links.len()],
                ));
                cx.notify();
                return;
            }
            Command::Remove | Command::DeleteFile => {
                self.remove(&chosen, command == Command::DeleteFile, cx);
                cx.notify();
                return;
            }
            Command::OpenFile
            | Command::OpenWith
            | Command::Preview
            | Command::OpenFolder
            | Command::ExtractHere
            | Command::ExtractTo => {
                self.act_on_file(command, &chosen, cx);
                return;
            }
            Command::SelectAll => {
                self.select_all(cx);
                return;
            }
            Command::Import => {
                self.import(cx);
                return;
            }
            Command::Export => {
                self.export(cx);
                return;
            }
            Command::Help => {
                self.help();
                cx.notify();
                return;
            }
            Command::ToggleCategories => {
                self.show_sidebar = !self.show_sidebar;
                cx.notify();
                return;
            }
            Command::ToggleToolbar => {
                self.show_toolbar = !self.show_toolbar;
                cx.notify();
                return;
            }
            Command::SortByName
            | Command::SortBySize
            | Command::SortByStatus
            | Command::SortByTimeLeft
            | Command::SortByRate
            | Command::SortByDate
            | Command::SortByDescription => {
                let column = match command {
                    Command::SortByName => Column::Name,
                    Command::SortBySize => Column::Size,
                    Command::SortByStatus => Column::Status,
                    Command::SortByTimeLeft => Column::TimeLeft,
                    Command::SortByRate => Column::Rate,
                    Command::SortByDate => Column::LastTry,
                    _ => Column::Description,
                };
                self.sort_by(column);
                cx.notify();
                return;
            }
            Command::ShowProgress => {
                let ids: Vec<dm_core::Id> = chosen
                    .iter()
                    .filter(|r| r.state != State::Completed)
                    .map(|r| r.id)
                    .collect();
                for id in ids {
                    self.progress_closed.remove(&id);
                    self.show_progress(id, cx);
                }
                return;
            }
            Command::SiteFiles => {
                crate::grabber::open(cx.entity().downgrade(), cx);
                return;
            }
            Command::Scheduler => {
                crate::scheduler::open(cx.entity().downgrade(), self.current_queue(), cx);
                return;
            }
            Command::SpeedLimiterSettings => {
                crate::limiter::open(cx.entity().downgrade(), cx);
                return;
            }
            Command::SpeedLimiterToggle
            | Command::SpeedLow
            | Command::SpeedMedium
            | Command::SpeedHigh
            | Command::SpeedUnlimited => {
                self.speed(command);
                cx.notify();
                return;
            }
            Command::StartQueue | Command::StopQueue => {
                let queue = self.current_queue();
                self.run_on_queue(command, &queue, cx);
                return;
            }
            Command::AddUrl => {
                crate::addurl::open(cx.entity().downgrade(), cx);
                return;
            }
            Command::CopyDetails => {
                let settings = self.backend.settings();
                if let Some((text, count)) = dm_desktop_base::details::blocks_for(
                    &chosen,
                    settings.connections,
                    &settings.download_dir,
                ) {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    self.say(dm_desktop_base::i18n::tn(
                        "Copied the details of {} download.||Copied the details of {} downloads.",
                        count as u64,
                        &[&count],
                    ));
                }
                cx.notify();
                return;
            }
            _ => {}
        }
        let queue = self.current_queue();
        let ran = dm_shell::app::commands::verb(command, &self.backend, &chosen, &queue);
        self.read();
        // Resume on rows the schedule or the download limit holds: each of
        // the others went on, and these are asked about.
        if let Some(dm_shell::app::commands::Ran::WentOn { held }) = ran {
            self.ask_outside(held, cx);
        }
        cx.notify();
    }

    /// Adds `urls` with the options Download details chose, as the person's
    /// own; the rows are read again.
    pub fn add(
        &mut self,
        urls: Vec<String>,
        with: dm_core::AddOptions,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let later = with.later;
        let items = urls.into_iter().map(|u| (u, with.clone())).collect();
        let added = self.session.add(&self.backend, items);
        if let Some(id) = added.rows.first().map(|(id, _, _)| *id) {
            // The new row is the one chosen, as the person expects of a row
            // just added.
            self.selected.clear();
            self.selected.insert(id);
        }
        if let Some((failed, e)) = added.partly() {
            // The rest went in; the ones that did not are said, not lost.
            self.say(match failed {
                1 => format!("One of the links was not added: {e}"),
                n => format!("{n} of the links were not added: {e}"),
            });
        }
        self.read();
        // Its progress window opens as it starts, where that is asked for
        // (B63), for one download at a time.
        if let [(id, _, false)] = added.rows.as_slice() {
            if self.backend.settings().show_progress {
                self.show_progress(*id, cx);
            }
        }
        // Start now while the schedule or the download limit holds
        // downloads: asked, as Resume asks, whether to start it outside the
        // schedule (asked of the schedule itself: the list, just read, may
        // hold no row waiting on it yet).
        let settings = self.backend.settings();
        let shut = settings.schedule.enabled && !settings.schedule.open();
        if !later && (shut || self.backend.allowance_spent().is_some()) {
            let ids = added.rows.iter().map(|(id, _, _)| *id).collect();
            self.ask_outside(ids, cx);
        }
        cx.notify();
        added.outcome().map(|_| ())
    }

    /// "Start outside the schedule?" for the rows `ids` the schedule or the
    /// download limit holds: Start now starts them this once; Keep waiting
    /// leaves them.
    pub fn ask_outside(&mut self, ids: Vec<dm_core::Id>, cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        let settings = self.backend.settings();
        let mut categories = settings.categories();
        // The name the list shows: "big.zip" was asked about while the row
        // read "big (1).zip".
        let names: Vec<String> = ids
            .iter()
            .filter_map(|id| self.rows.iter().find(|r| r.id == *id))
            .map(|row| {
                dm_shell::app::names::name_it_will_get_after(
                    &self.rows,
                    row,
                    &settings.download_dir,
                    &mut categories,
                    None,
                )
            })
            .collect();
        let why = dm_desktop_base::held::held_because(&settings, self.backend.allowance_spent());
        let ask = dm_desktop_base::asks::outside(&names, why);
        let asked = crate::question::Asked {
            title: ask.title.into(),
            lines: std::iter::once(ask.sentence)
                .chain(ask.why)
                .chain(std::iter::once(ask.question.to_string()))
                .map(Into::into)
                .collect(),
            named: None,
            detail: None,
            toggle: None,
            // Keep waiting beside Start now, the default, rightmost.
            buttons: vec![ask.start.into(), ask.wait.into()],
            default: 0,
            escape: 1,
        };
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, _, cx| {
            if button != 0 {
                return;
            }
            let _ = view.update(cx, |view, cx| {
                for id in &ids {
                    view.backend.start_outside_schedule(*id);
                }
                view.read();
                cx.notify();
            });
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// Adds downloads with options of their own (Download links' each has
    /// its name, its description, its sign-in), as the person's own; how
    /// many went in.
    pub fn add_items(
        &mut self,
        items: Vec<(String, dm_core::AddOptions)>,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let added = self.session.add(&self.backend, items);
        if let Some((failed, e)) = added.partly() {
            // The rest went in; the ones that did not are said, not lost.
            self.say(match failed {
                1 => format!("One of the links was not added: {e}"),
                n => format!("{n} of the links were not added: {e}"),
            });
        }
        self.read();
        cx.notify();
        added.outcome()
    }

    /// A handed-over download from `site` was cancelled: the second asks
    /// whether to leave that site's downloads to the browser (counted by the
    /// application, `handoff::Cancels`).
    pub fn hand_off_cancelled(&mut self, site: &str, cx: &mut Context<Self>) {
        let settings = self.backend.settings();
        if !self
            .cancelled_hand_offs
            .cancelled(site, &settings.catch.skip_hosts)
        {
            return;
        }
        let asked = crate::question::Asked {
            title: dm_desktop_base::i18n::t("Leave this site to the browser?").into(),
            lines: vec![dm_desktop_base::i18n::tf(
                "You cancelled two downloads from {}. Leave its downloads to the browser?",
                &[&site],
            )
            .into()],
            named: None,
            detail: None,
            toggle: None,
            buttons: vec![
                dm_desktop_base::i18n::t("Leave them to the browser").into(),
                dm_desktop_base::i18n::t("Keep handing them over").into(),
            ],
            default: 0,
            escape: 1,
        };
        let site = site.to_string();
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, _, cx| {
            if button != 0 {
                return;
            }
            let _ = view.update(cx, |view, _| {
                let mut wanted = view.backend.settings();
                if !wanted.catch.skip_hosts.contains(&site) {
                    wanted.catch.skip_hosts.push(site);
                }
                if let Err(e) = view.backend.set_settings(wanted) {
                    view.problems.push(e);
                }
            });
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// When done's countdown, shown once it starts: "dm will sleep in 30
    /// seconds.", Cancel (the default) and Now. It closes once the countdown
    /// is over or cancelled.
    pub(crate) fn show_countdown(&mut self, cx: &mut Context<Self>) {
        if self.session.countdown.is_none() {
            self.countdown_shown = false;
            return;
        }
        if self.countdown_shown {
            return;
        }
        self.countdown_shown = true;
        let Some(asked) = countdown_question(self) else {
            return;
        };
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, _, cx| {
            let _ = view.update(cx, |view, cx| {
                view.countdown_shown = false;
                match (button, view.session.countdown) {
                    // Now: the action at once.
                    (1, Some((action, _))) => {
                        view.session.countdown = Some((action, std::time::Instant::now()));
                        view.countdown_now(cx);
                    }
                    _ => view.session.countdown = None,
                }
                cx.notify();
            });
        });
        let live: crate::question::Live = Box::new(countdown_question);
        self.sheet(asked, None, Some(live), answer, cx);
    }

    /// Clears the search box: every row of the place shown again.
    pub fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.update(cx, |f, cx| f.set_value("", window, cx));
        self.filter_text.clear();
        self.reshow();
        cx.notify();
    }

    /// Find (⌘F): the keyboard to the top bar's search box, its words
    /// chosen, as a code editor's ⌘P puts it in its command centre.
    pub fn find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.update(cx, |f, cx| {
            f.focus(window, cx);
            f.select_all(window, cx);
        });
    }

    /// Find next (`forward`) or previous: the next row shown whose name
    /// holds the search box's words, going round the list
    /// (`dm_desktop_base::find`), chosen and brought into view.
    pub fn find_step(&mut self, forward: bool) {
        use dm_desktop_base::i18n::tf;
        let needle = self.filter_text.trim().to_lowercase();
        if needle.is_empty() {
            return;
        }
        let names: Vec<String> = self
            .shown
            .iter()
            .map(|&i| dm_shell::app::names::name_of(&self.rows[i]))
            .collect();
        let from = self
            .shown
            .iter()
            .position(|&i| self.selected.contains(&self.rows[i].id));
        match dm_desktop_base::find::next_match(&names, from, &needle, forward) {
            Some(at) => {
                let row = &self.rows[self.shown[at]];
                self.selected = std::collections::BTreeSet::from([row.id]);
                self.scroll.scroll_to_item(at, gpui::ScrollStrategy::Center);
                self.say(tf("Found {}.", &[&names[at]]));
            }
            None => self.say(tf("Nothing is called “{}”.", &[&self.filter_text])),
        }
    }

    /// Start Queue or Stop Queue for `queue`, and what came of it said:
    /// how many started (or that the schedule holds them, or that nothing
    /// was waiting) or stopped. Checking the Recheck queue is said when it
    /// is done.
    pub fn run_on_queue(
        &mut self,
        command: dm_shell::app::commands::Command,
        queue: &str,
        cx: &mut Context<Self>,
    ) {
        use dm_desktop_base::i18n::{tf, tn};
        use dm_shell::app::commands::{Command, Ran};
        let ran = dm_shell::app::commands::verb(command, &self.backend, &[], queue);
        self.read();
        let name = dm_desktop_base::i18n::name(queue);
        match (command, ran) {
            (Command::StartQueue, ran) => {
                let n = match ran {
                    Some(Ran::QueueStarted(n)) => n,
                    _ => 0,
                };
                let settings = self.backend.settings();
                // Its own sound, only where the person chose a file.
                if n > 0 {
                    if let Some(path) = settings
                        .sounds
                        .get("queue_started")
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                    {
                        self.backend.platform.play_file(&path);
                    }
                }
                let schedule = settings.schedule_for(queue);
                let lets = !schedule.enabled || schedule.open();
                self.say(if n == 0 {
                    tf("Nothing in {} was waiting to start.", &[&name])
                } else if !lets {
                    tf(
                        "{} is waiting for the schedule; it starts by itself when the schedule opens.",
                        &[&name],
                    )
                } else {
                    tn(
                        "Started {} download in {}.||Started {} downloads in {}.",
                        n as u64,
                        &[&n, &name],
                    )
                });
                if queue == dm_core::RECHECK_QUEUE {
                    dm_shell::app::commands::recheck(&self.backend, self.rechecked.clone());
                }
            }
            (_, ran) => {
                let n = match ran {
                    Some(Ran::QueueStopped(n)) => n,
                    _ => 0,
                };
                self.say(tn(
                    "Stopped {} download in {}.||Stopped {} downloads in {}.",
                    n as u64,
                    &[&n, &name],
                ));
            }
        }
        cx.notify();
    }

    /// The Speed limiter on or off, or a preset (Low, Medium, High,
    /// Unlimited), as `commands::limited` says, and said.
    fn speed(&mut self, command: dm_shell::app::commands::Command) {
        use dm_desktop_base::i18n::{t, tf};
        let now = self.backend.settings();
        let wanted =
            dm_shell::app::commands::limited(command, &now, &mut self.last_limit).unwrap_or(now);
        match self.backend.set_settings(wanted) {
            Ok(after) if after.speed_limit > 0 => {
                self.say(tf("Speed limiter on: {} KB/s.", &[&after.speed_limit]))
            }
            Ok(_) => self.say(t("Speed limiter off.").into()),
            Err(e) => self.say(e),
        }
        self.session.forget_limit();
    }

    /// The menu bar's queue submenus, built again when the queues change.
    pub(crate) fn refresh_menus(&mut self, cx: &mut Context<Self>) {
        let queues = self.backend.queues();
        if queues != self.menu_queues {
            crate::menus::set(cx, &queues);
            self.menu_queues = queues;
        }
    }

    /// The Recheck queue's counts, once its check is done.
    pub(crate) fn say_rechecked(&mut self) {
        let done = dm_core::lock(&self.rechecked).take();
        if let Some((checked, changed)) = done {
            self.say(dm_desktop_base::i18n::tn(
                "Checked {} finished file with its server. Changed since, and downloaded again: {}.||Checked {} finished files with their servers. Changed since, and downloaded again: {}.",
                checked as u64,
                &[&checked, &changed],
            ));
        }
    }

    /// Adds what a search found. With `keep_layout` each file goes in a
    /// folder under the download folder that mirrors its address, one add
    /// for each folder; otherwise all of them in one.
    pub fn add_found(
        &mut self,
        urls: Vec<String>,
        with: dm_core::AddOptions,
        keep_layout: bool,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        if !keep_layout {
            let items = urls.into_iter().map(|u| (u, with.clone())).collect();
            return self.add_items(items, cx);
        }
        let root = self.backend.settings().download_dir;
        let mut items = Vec::new();
        for (folder, group) in dm_shell::app::search::by_site_folder(&urls, &root) {
            let mut here = with.clone();
            if folder.is_some() {
                here.save_dir = folder;
            }
            items.extend(group.into_iter().map(|u| (u, here.clone())));
        }
        self.add_items(items, cx)
    }

    /// Runs a saved search now, in the background; what it finds is taken
    /// in on a later wake (`take_searches`).
    pub fn run_search(&mut self, search: dm_shell::lists::Search) {
        let mut search = search;
        // The login Options › Sign-ins keeps for the site, where it keeps one.
        search.page = self.backend.settings().with_login(&search.page);
        let results = self.search_results.clone();
        let name = search.name.clone();
        dm_shell::app::search::crawl_later(&self.backend, search, move |found| {
            dm_core::lock(&results).push((name, found));
            dm_shell::app::changes::changed();
        });
    }

    /// Takes in what finished saved searches found (their new files added
    /// for later, said; what they have seen kept), and starts any saved
    /// search whose time has come, looked at once a minute.
    pub(crate) fn take_searches(&mut self, cx: &mut Context<Self>) {
        use dm_desktop_base::i18n::{tf, tn};
        let done: Vec<_> = std::mem::take(&mut *dm_core::lock(&self.search_results));
        for (name, found) in done {
            let settings = self.backend.settings();
            match found {
                Ok(found) => {
                    let saved = settings.site_searches.iter().find(|s| s.name == name);
                    let seen: Vec<String> = saved.map(|s| s.seen.clone()).unwrap_or_default();
                    let keep_layout = saved.is_some_and(|s| s.keep_layout);
                    // New: neither in the list nor found by this search
                    // before (`dm_shell::app::search`).
                    let finds = dm_shell::app::search::sort_finds(&found.files, &self.rows, &seen);
                    let new = finds.new;
                    let mut remember = finds.listed;
                    if new.is_empty() {
                        self.say(tf("The search {} found nothing new.", &[&name]));
                    } else {
                        let with = dm_core::AddOptions {
                            later: true,
                            queue: dm_core::MAIN_QUEUE.into(),
                            ..Default::default()
                        };
                        let added = self.add_found(new.clone(), with, keep_layout, cx);
                        let now = dm_shell::app::search::listed(&self.rows);
                        remember.extend(
                            new.iter()
                                .map(|f| dm_shell::lists::bare(f))
                                .filter(|f| now.contains(f)),
                        );
                        match added {
                            Ok(n) => self.say(tn(
                                "Added {} download to {}.||Added {} downloads to {}.",
                                n as u64,
                                &[&n, &dm_desktop_base::i18n::name(dm_core::MAIN_QUEUE)],
                            )),
                            Err(e) => self.problems.push(format!(
                                "the search {name} found files that could not be added: {e}"
                            )),
                        }
                    }
                    let settings = self.backend.settings();
                    if let Some(wanted) =
                        dm_shell::app::search::seen_saved(&settings, &name, remember)
                    {
                        if let Err(e) = self.backend.set_settings(wanted) {
                            self.problems.push(format!(
                                "what the search {name} has seen could not be saved: {e}"
                            ));
                        }
                    }
                }
                Err(e) => self.problems.push(format!("the search {name} failed: {e}")),
            }
        }
        let now = std::time::Instant::now();
        if self
            .searches_checked
            .is_some_and(|at| now.duration_since(at) < std::time::Duration::from_secs(60))
        {
            return;
        }
        self.searches_checked = Some(now);
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let settings = self.backend.settings();
        let Some((due, wanted)) = dm_shell::app::search::due_now(&settings, epoch) else {
            return;
        };
        if let Err(e) = self.backend.set_settings(wanted) {
            // Still run, and not again for a minute; but said, since an
            // unsaved last run means the search runs again at every launch.
            self.problems.push(format!(
                "when the saved searches last ran could not be saved: {e}"
            ));
        }
        for search in due {
            self.run_search(search);
        }
    }

    /// The progress window of `id`, opened (or brought forward).
    pub fn show_progress(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        if let Some(handle) = self.progress_windows.get(&id) {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        let nth = self.progress_windows.len();
        let me = cx.entity().downgrade();
        let main = self.main;
        cx.defer(move |cx| {
            if let Some(handle) = crate::progress::open(me.clone(), id, nth, main, cx) {
                let _ = me.update(cx, |view, _| {
                    view.progress_windows.insert(id, handle);
                });
            }
        });
    }

    /// Whether a person closed the progress window of `id`.
    pub(crate) fn progress_closed_has(&self, id: dm_core::Id) -> bool {
        self.progress_closed.contains(&id)
    }

    /// Closes the progress window of `id`, where one is open.
    fn close_progress(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        if let Some(handle) = self.progress_windows.remove(&id) {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// Progress windows a person closed: forgotten, and not opened again by
    /// themselves.
    pub(crate) fn prune_progress(&mut self, cx: &mut Context<Self>) {
        let open = cx.windows();
        let gone: Vec<dm_core::Id> = self
            .progress_windows
            .iter()
            .filter(|(_, handle)| !open.contains(handle))
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.progress_windows.remove(&id);
            self.progress_closed.insert(id);
        }
        if self.complete_window.is_some_and(|w| !open.contains(&w)) {
            self.complete_window = None;
            self.complete.clear();
        }
    }

    /// Downloads that finished: their progress windows close, and
    /// "Download complete" lists them, where that is asked for.
    pub(crate) fn finished(&mut self, ids: &[dm_core::Id], cx: &mut Context<Self>) {
        let show = self.backend.settings().show_complete;
        for id in ids {
            self.close_progress(*id, cx);
            if show {
                self.complete.insert(*id);
            }
        }
        if show && !self.complete.is_empty() && self.complete_window.is_none() {
            let me = cx.entity().downgrade();
            cx.defer(move |cx| {
                let handle = crate::complete::open(me.clone(), cx);
                let _ = me.update(cx, |view, _| view.complete_window = handle);
            });
        }
    }

    /// "Download complete" closed: a ticked "Don't show this window again"
    /// kept, and said.
    pub fn complete_closed(&mut self, never: bool, cx: &mut Context<Self>) {
        self.complete.clear();
        self.complete_window = None;
        if never {
            let mut wanted = self.backend.settings();
            wanted.show_complete = false;
            match self.backend.set_settings(wanted) {
                Ok(_) => self.say(
                    dm_desktop_base::i18n::t(
                        "This window won't open again. Turn it on in Options › General.",
                    )
                    .into(),
                ),
                Err(e) => self.say(e),
            }
        }
        cx.notify();
    }

    /// A file acted on in "Download complete": it leaves the list, and the
    /// window with the last.
    pub fn complete_done(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        self.complete.remove(&id);
        if self.complete.is_empty() {
            if let Some(handle) = self.complete_window.take() {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        }
        cx.notify();
    }

    /// Where an opener says what went wrong, from its thread.
    pub(crate) fn opener_said(&self) -> std::sync::Arc<std::sync::Mutex<Option<String>>> {
        self.opener_said.clone()
    }

    /// Opens a file with its usual app, off the window's thread.
    pub fn open_path(&self, path: &str) {
        let said = self.opener_said.clone();
        self.backend
            .platform
            .open_later(path, Box::new(move |why| *dm_core::lock(&said) = Some(why)));
    }

    /// Shows a file in the file manager, off the window's thread.
    pub fn reveal_path(&self, path: &str) {
        let said = self.opener_said.clone();
        self.backend
            .platform
            .reveal_later(path, Box::new(move |why| *dm_core::lock(&said) = Some(why)));
    }

    /// Open with…: the app chosen in the system's own panel.
    pub fn open_path_with(&mut self, path: &str) {
        let chosen = self.backend.platform.choose_app();
        if let Some(app) = chosen {
            if let Err(e) = self.backend.platform.open_with(path, &app) {
                self.say(e);
            }
        }
    }

    /// What an opener said went wrong, from its thread.
    pub(crate) fn say_opener(&mut self) {
        let said = dm_core::lock(&self.opener_said).take();
        if let Some(why) = said {
            self.say(why);
        }
    }

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
    fn watch_asks(&mut self, cx: &mut Context<Self>) {
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
    fn promote_asks(&mut self, cx: &mut Context<Self>) {
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
    fn answered(&mut self, question: dm_shell::app::questions::Question, yes: bool) {
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
        self.backend.flush();
        for (handle, _, dialog) in crate::screen::Screens::all(cx) {
            if dialog {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
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
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, ticked, cx| {
            answer(button, ticked, cx);
            let _ = view.update(cx, |view, cx| {
                view.sheet_open = false;
                view.show_sheets(cx);
            });
        });
        self.sheets.push_back((asked, reword, live, answer));
        self.show_sheets(cx);
    }

    /// Opens the next waiting question, when none is open and the main
    /// window is in front.
    fn show_sheets(&mut self, cx: &mut Context<Self>) {
        while !self.sheet_open && cx.active_window() == Some(self.main) {
            let Some((asked, reword, live, answer)) = self.sheets.pop_front() else {
                return;
            };
            let live = live.map(|live| (live, cx.entity()));
            self.sheet_open = crate::question::ask(asked, reword, live, answer, cx);
        }
    }

    /// Whether `command` can be run now: the shell carries it out, and it
    /// applies to the rows chosen (`commands::applies`), as the egui
    /// window greys its buttons and menu items.
    pub fn enabled(&self, command: dm_shell::app::commands::Command) -> bool {
        if !crate::menus::carried_out(command) {
            return false;
        }
        let chosen = self.chosen();
        dm_shell::app::commands::applies(command, &chosen, &self.rows, false, &Looks)
    }

    /// The rows chosen, in the list's order.
    pub fn chosen(&self) -> Vec<&Entry> {
        self.shown
            .iter()
            .filter_map(|&r| self.rows.get(r))
            .filter(|r| self.selected.contains(&r.id))
            .map(|r| &**r)
            .collect()
    }

    /// The queue Start and Stop Queue act on: the one shown, else the Main
    /// queue.
    pub fn current_queue(&self) -> String {
        match &self.place {
            Place::Queue(name) => name.clone(),
            _ => dm_core::MAIN_QUEUE.to_string(),
        }
    }

    /// One read of the application: the rows and what follows from them.
    pub(crate) fn read(&mut self) {
        dm_shell::app::changes::take();
        let settings = self.backend.settings();
        let read = self.session.read(&self.backend, &settings);
        // What it detected and the problems it met are the tick's.
        self.detected.push(read.detected);
        self.problems.extend(read.problems);
        // What the read told beside the rows, which a row's words are said
        // from (ranges tried again, swarms, files taken by a later row, the
        // download limit), as the egui window sets it.
        dm_desktop_base::rows::set_retrying(read.snapshot.retrying);
        dm_desktop_base::rows::set_swarms(read.snapshot.swarms);
        dm_desktop_base::rows::set_limit_holds(read.snapshot.allowance_spent);
        self.rows = read.snapshot.rows;
        // A finished row's size is its file's, read once from the disk.
        self.saved.apply(&mut self.rows);
        dm_desktop_base::rows::set_superseded(&self.rows);
        // A row taken off the list is no longer chosen.
        let ids: std::collections::HashSet<dm_core::Id> = self.rows.iter().map(|r| r.id).collect();
        self.selected.retain(|id| ids.contains(id));
        self.places.place_rows(&self.rows, &settings.categories());
        self.reshow();
    }

    /// Shows the rows of `place`.
    pub fn choose(&mut self, place: Place) {
        self.place = place;
        self.reshow();
    }

    /// A click on the row at `position` in the list, with ⌘ (`toggle`) or
    /// ⇧ (`range`) held, as a macOS list takes it.
    pub fn click_row(&mut self, position: usize, toggle: bool, range: bool) {
        let Some(id) = self.id_at(position) else {
            return;
        };
        if range {
            let from = self
                .anchor
                .and_then(|a| self.position_of(a))
                .unwrap_or(position);
            let (low, high) = (from.min(position), from.max(position));
            self.selected = (low..=high).filter_map(|p| self.id_at(p)).collect();
        } else if toggle {
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
            self.anchor = Some(id);
        } else {
            self.selected = [id].into();
            self.anchor = Some(id);
        }
    }

    /// A right-click on the row at `position`, at `at`: the row is chosen
    /// if it was not (a right-click on a chosen row keeps the choice), and
    /// the row menu opens there.
    pub fn open_menu(&mut self, position: usize, at: gpui::Point<gpui::Pixels>) {
        if let Some(id) = self.id_at(position) {
            if !self.selected.contains(&id) {
                self.selected = [id].into();
                self.anchor = Some(id);
            }
            self.menu = Some(at);
        }
    }

    /// Remove (or Delete file, `deleting_files`) on the rows chosen. Finished
    /// rows only leave the list, nothing lost, Undo on offer for a moment; any
    /// other is asked about first, as the egui window asks.
    fn remove(&mut self, chosen: &[Entry], deleting_files: bool, cx: &mut Context<Self>) {
        let ids: Vec<dm_core::Id> = chosen.iter().map(|r| r.id).collect();
        if !deleting_files && chosen.iter().all(|r| r.state == State::Completed) {
            let rows = self.backend.finished_rows(&ids);
            self.backend.remove_many(&ids);
            let n = self.undo.keep(rows);
            if n > 0 {
                self.say(dm_desktop_base::i18n::tn(
                    "Removed {} download from the list.||Removed {} downloads from the list.",
                    n as u64,
                    &[&n],
                ));
            }
            self.read();
            return;
        }
        // The finished files still on disk that could go with their rows.
        let files: Vec<String> = chosen
            .iter()
            .filter(|r| r.state == State::Completed && std::path::Path::new(&r.path).is_file())
            .map(|r| r.path.clone())
            .collect();
        let named = match chosen {
            [only] => Some(dm_shell::app::names::name_of(only)),
            _ => None,
        };
        let rows: Vec<Entry> = chosen.to_vec();
        let count_files = files.len();
        let words = move |ticked: bool| {
            let refs: Vec<&Entry> = rows.iter().collect();
            let ask = dm_desktop_base::asks::remove(
                &refs,
                count_files,
                ticked,
                deleting_files,
                named.clone(),
            );
            crate::question::Asked {
                title: ask.title.into(),
                lines: std::iter::once(ask.sentence)
                    .chain(ask.files_sentence)
                    .map(Into::into)
                    .collect(),
                named: Some(ask.named.into()),
                detail: None,
                toggle: ask.toggle.map(Into::into),
                buttons: vec![ask.keep.into(), ask.go.into()],
                default: 0,
                escape: 0,
            }
        };
        let asked = words(false);
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, delete, cx| {
            if button == 1 {
                let _ = view.update(cx, |view, cx| {
                    view.removed(&ids, &files, delete);
                    cx.notify();
                });
            }
        });
        self.sheet(asked, Some(Box::new(words)), None, answer, cx);
    }

    /// The rows `ids` taken off the list, their finished `files` with them
    /// to the Trash where `delete`; the choice goes to the row after them.
    fn removed(&mut self, ids: &[dm_core::Id], files: &[String], delete: bool) {
        let mut failed: Vec<String> = Vec::new();
        if delete {
            for path in files {
                // A finished torrent still shared lets go of its files first.
                dm_core::stop_sharing(std::path::Path::new(path));
                if let Err(e) = self.backend.platform.trash(path) {
                    failed.push(format!("{path}: {e}"));
                }
            }
        }
        // The row after those removed, or the one before where they were
        // the last, is chosen next, so the keys have somewhere to go on from.
        let shown: Vec<dm_core::Id> = (0..self.shown.len())
            .filter_map(|p| self.id_at(p))
            .collect();
        let next = shown
            .iter()
            .rposition(|id| ids.contains(id))
            .and_then(|last| {
                shown[last + 1..]
                    .iter()
                    .find(|id| !ids.contains(id))
                    .or_else(|| shown[..last].iter().rev().find(|id| !ids.contains(id)))
                    .copied()
            });
        self.backend.remove_many(ids);
        self.read();
        self.selected = next.into_iter().collect();
        self.anchor = next;
        if !failed.is_empty() {
            self.say(dm_desktop_base::i18n::tf(
                "Could not delete {}",
                &[&failed.join("; ")],
            ));
        }
    }

    /// Puts back the rows the last Remove took off, while Undo is on offer.
    pub fn undo_remove(&mut self) {
        if let Some(ids) = self.undo.put_back(&self.backend) {
            let n = ids.len();
            self.read();
            self.selected = ids.into_iter().collect();
            self.say(dm_desktop_base::i18n::tn(
                "Put back {} download.||Put back {} downloads.",
                n as u64,
                &[&n],
            ));
        }
    }

    /// Says `sentence` in the status bar.
    pub fn say(&mut self, sentence: String) {
        self.said = Some((sentence, std::time::Instant::now()));
    }

    /// Closes the row menu.
    pub fn close_menu(&mut self) {
        self.menu = None;
    }

    /// Moves the choice one row up (`-1`) or down (`1`), as the arrow keys
    /// do, keeping the row on screen.
    pub fn step(&mut self, by: isize) {
        let count = self.shown.len();
        if count == 0 {
            return;
        }
        let now = self
            .anchor
            .filter(|a| self.selected.contains(a))
            .and_then(|a| self.position_of(a));
        let to = match now {
            Some(p) => (p as isize + by).clamp(0, count as isize - 1) as usize,
            None if by < 0 => count - 1,
            None => 0,
        };
        if let Some(id) = self.id_at(to) {
            self.selected = [id].into();
            self.anchor = Some(id);
            self.scroll
                .scroll_to_item(to, gpui::ScrollStrategy::Nearest);
        }
    }

    /// The id of the row at `position` in the list as shown.
    pub(crate) fn id_at(&self, position: usize) -> Option<dm_core::Id> {
        self.shown
            .get(position)
            .and_then(|&r| self.rows.get(r))
            .map(|r| r.id)
    }

    /// Where the row `id` is in the list as shown.
    fn position_of(&self, id: dm_core::Id) -> Option<usize> {
        self.shown
            .iter()
            .position(|&r| self.rows.get(r).is_some_and(|row| row.id == id))
    }

    /// Sorts by `column`: up, then the other way on a second click, as
    /// the egui window's header does.
    pub fn sort_by(&mut self, column: Column) {
        self.sort = match self.sort {
            Some((c, up)) if c == column => Some((c, !up)),
            _ => Some((column, true)),
        };
        self.reshow();
    }

    /// Which rows the list shows, in order: those in the place chosen,
    /// sorted as chosen.
    fn reshow(&mut self) {
        let categories = self.backend.settings().categories();
        let mut shown: Vec<usize> = (0..self.rows.len())
            .filter(|&i| {
                self.places.within(&self.place, &self.rows[i], &categories)
                    && self.places.matches_filter(&self.filter_text, &self.rows[i])
            })
            .collect();
        if let Some((column, up)) = self.sort {
            let rows = &self.rows;
            shown.sort_by(|&a, &b| {
                let order = dm_desktop_base::columns::compare(column, &rows[a], &rows[b]);
                if up {
                    order
                } else {
                    order.reverse()
                }
            });
        }
        self.shown = shown;
    }
}

/// What the application asks of the window about a row, from what the
/// last read told beside the rows (`dm_desktop_base::rows`).
struct Looks;

impl dm_shell::app::commands::Looks for Looks {
    fn retrying(&self, id: dm_core::Id) -> bool {
        dm_desktop_base::rows::retrying(id).is_some()
    }

    fn gone(&self, row: &Entry) -> bool {
        dm_desktop_base::rows::gone(row)
    }

    fn plays(&self, row: &Entry) -> bool {
        matches!(
            dm_shell::app::names::kind_of(row),
            Some(dm_core::Category::Videos | dm_core::Category::Audio)
        )
    }
}

/// A row's picture of its file's kind, and the kind's colour (B45): told
/// apart by shape (a play mark, a note, lines, a window, a zip, a hill and a
/// sun, a plain sheet), in the egui window's colours.
pub(crate) fn kind_picture(kind: Option<dm_core::Category>) -> (&'static str, gpui::Rgba) {
    use dm_core::Category as K;
    match kind {
        Some(K::Videos) => ("icons/kind-videos.svg", gpui::rgb(0x8e44ad)),
        Some(K::Audio) => ("icons/kind-audio.svg", gpui::rgb(0xe67e22)),
        Some(K::Documents) => ("icons/kind-documents.svg", gpui::rgb(0x2b6cc4)),
        Some(K::Apps) => ("icons/kind-apps.svg", gpui::rgb(0x3d8b37)),
        Some(K::Archives) => ("icons/kind-archives.svg", gpui::rgb(0xb7931f)),
        Some(K::Pictures) => ("icons/kind-pictures.svg", gpui::rgb(0xc0395a)),
        _ => ("icons/kind-other.svg", gpui::rgb(0x7f8c8d)),
    }
}

/// What a row's line says, in the shell: its name and its state.
pub(crate) fn line(row: &Entry) -> (SharedString, SharedString) {
    let name = if row.filename.is_empty() {
        dm_shell::app::names::provisional(&row.url)
    } else {
        row.filename.clone()
    };
    let state = match &row.state {
        State::Running => match row.progress.total {
            Some(total) if total > 0 => {
                format!("{}%", row.progress.downloaded.saturating_mul(100) / total)
            }
            _ => dm_desktop_base::format::bytes(row.progress.downloaded),
        },
        State::Failed(why) => format!("Failed: {why}"),
        other => format!("{other:?}"),
    };
    (name.into(), state.into())
}

impl Render for Downloads {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        div()
            .id("downloads")
            .role(Role::Application)
            .aria_label("dm")
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, run: &crate::menus::RunOnQueue, _, cx| {
                this.run_on_queue(run.command, &run.queue, cx)
            }))
            .on_action(cx.listener(|this, run: &crate::menus::Run, window, cx| {
                use dm_shell::app::commands::Command;
                match run.0 {
                    // The search box is the window's: these need it.
                    Command::Find => this.find(window, cx),
                    Command::FindNext => {
                        this.find_step(true);
                        cx.notify();
                    }
                    Command::FindPrevious => {
                        this.find_step(false);
                        cx.notify();
                    }
                    command => this.run(command, cx),
                }
            }))
            .on_action(
                cx.listener(|this, _: &crate::menus::SelectPrevious, _, cx| {
                    this.step(-1);
                    cx.notify();
                }),
            )
            .on_action(cx.listener(|this, _: &crate::menus::SelectNext, _, cx| {
                this.step(1);
                cx.notify();
            }))
            .bg(theme.background)
            .text_color(theme.text)
            // The system's size for a list's rows and every control.
            .text_size(gpui::px(crate::native::TEXT))
            .child(crate::titlebar::titlebar(self, theme, &screen, window, cx))
            .when(self.show_toolbar, |root| {
                root.child(crate::toolbar::toolbar(self, theme, &screen, window, cx))
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(self.show_sidebar, |row| {
                        row.child(crate::sidebar::sidebar(self, theme, &screen, window, cx))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            // Problems from outside the list, each until
                            // dismissed, above it.
                            .children(self.problems.iter().enumerate().map(|(i, problem)| {
                                gpui::div()
                                    .flex()
                                    .items_center()
                                    .gap(gpui::px(crate::native::LABEL_GAP))
                                    .px(gpui::px(12.0))
                                    .py(gpui::px(6.0))
                                    .border_b_1()
                                    .border_color(theme.separator)
                                    .child(gpui::div().flex_1().text_color(theme.warning).child(
                                        screen.label(dm_desktop_base::format::as_shown(problem)),
                                    ))
                                    .child(
                                        crate::native::button(
                                            ("dismiss", i),
                                            dm_desktop_base::i18n::t("Dismiss").into(),
                                            false,
                                            theme,
                                            &screen,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                if i < this.problems.len() {
                                                    this.problems.remove(i);
                                                }
                                                cx.notify();
                                            }),
                                        ),
                                    )
                            }))
                            .child(crate::table::header(self, theme, &screen, cx))
                            .child(crate::table::rows(self, theme, cx)),
                    ),
            )
            .child(crate::statusbar::statusbar(self, theme, &screen, cx))
            .when_some(self.menu, |root, at| {
                root.child(crate::rowmenu::menu(self, at, theme, &screen, cx))
            })
    }
}
