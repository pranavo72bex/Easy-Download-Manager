// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The download list as the shell draws it: the application read once each
//! time the window is woken (a queue change, coalesced over a quarter of a
//! second; an order or a signal at once; every
//! quarter of a second while a download runs, and once a second for the
//! schedules' and When done's clocks), and only the rows on screen laid out
//! (`uniform_list`), so a list of 10,000 costs what the screenful does. A
//! script's reading of the list is answered on the control port's own
//! thread (`port.rs`), the window neither woken nor drawn for it.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dm_core::{Entry, State};
use dm_desktop_base::boot::Wake;
use dm_desktop_base::session::Session;
use dm_shell::app::backend::Backend;
use gpui::{div, prelude::*, Context, FocusHandle, Role, Window};

use crate::window::screen::Screen;
use crate::window::theme::Theme;
use dm_desktop_base::columns::Column;
use dm_shell::app::places::Place;

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

/// What ended one of the window's waits.
enum Woke {
    /// Something woke it.
    By(Wake),
    /// Its timer ran out first.
    Clock,
    /// Nothing can wake it any more: dm is ending.
    Gone,
}

/// The next wake, or the timer's end, whichever comes first.
async fn woke(woken: &dm_desktop_base::wakes::Woken, timer: gpui::Task<()>) -> Woke {
    futures_lite::future::or(
        async { woken.recv().await.map_or(Woke::Gone, Woke::By) },
        async {
            timer.await;
            Woke::Clock
        },
    )
    .await
}

/// How long the window sleeps with nothing waking it (`clock_every`), with
/// a download `running` or a clock `ticking` (When done's countdown, the
/// clipboard watched).
fn clock(running: bool, ticking: bool) -> Duration {
    if running {
        RUNNING
    } else if ticking {
        TICKING
    } else {
        IDLE
    }
}

/// When the list's bars show, as each system's own lists show theirs: always on Windows and
/// Linux, under the pointer on macOS (gaps row U43).
fn scrollbar_mode() -> gpui_base::ScrollbarMode {
    if cfg!(target_os = "macos") {
        gpui_base::ScrollbarMode::Hover
    } else {
        gpui_base::ScrollbarMode::Always
    }
}

/// The rows' height, in points.
pub(crate) const ROW_HEIGHT: f32 = 24.0;

/// How often a window with nothing moving wakes of its own accord.
const IDLE: Duration = Duration::from_secs(30);

/// How often the window reads and draws the list while a download runs.
const RUNNING: Duration = Duration::from_millis(250);

/// How often the window wakes while a clock ticks: When done's countdown, the clipboard
/// watched.
const TICKING: Duration = Duration::from_secs(1);

/// How long after a change to the queue it is read, the burst within it taken as one.
const SOON: Duration = Duration::from_millis(250);

/// How often the saved searches are looked at for one that is due.
const SEARCHES_EVERY: Duration = Duration::from_secs(60);

/// How long a login launch waits for something to show its tray before its
/// window opens instead.
const HIDDEN_WAIT: Duration = Duration::from_secs(10);

mod commands;
mod complete;
mod keys;
mod later;
mod questions;
mod refresh;
mod removal;
mod rowline;
mod sections;
mod sorting;
mod state;

use questions::countdown_question;
use refresh::clicked;
use refresh::needs_drawing;
pub(crate) use rowline::{kind_picture, line};
use sorting::sort_command;

/// The main window's view: the application, the rows it last read, and what
/// the window keeps, a struct per concern (`list/state.rs`).
pub struct Downloads {
    pub(crate) backend: Backend,
    pub(crate) session: Session,
    pub(crate) rows: Vec<Arc<Entry>>,
    /// The words on screen, where the last frame laid them out.
    pub(crate) screen: Screen,
    /// The window's focus: where keys and the menu bar's commands arrive.
    pub(crate) focus: FocusHandle,
    /// The speed limit before the limiter was turned off, for turning it on.
    last_limit: u64,
    /// The list as it is seen: scrolled, the place, sorted, chosen, searched.
    pub(crate) list: state::ListView,
    /// What the last read worked out for drawing.
    pub(crate) known: state::Known,
    /// The questions asked, and those waiting.
    pub(crate) questions: state::Questions,
    /// The windows open beside this one, and its own frame.
    pub(crate) windows: state::OpenWindows,
    /// The row menu, a place's menu and the menu bar.
    pub(crate) menus: state::Menus,
    /// What of the window is shown, and how wide.
    pub(crate) layout: state::Layout,
    /// The status bar's words, the problems and Undo.
    pub(crate) status: state::Status,
    /// The tray, the taskbar button and the clipboard.
    pub(crate) system: state::System,
    /// The control port, and the window asked for from outside.
    pub(crate) port: state::Port,
    /// Work off the window's thread and what it brought back.
    pub(crate) threads: state::Threads,
}

impl Downloads {
    /// The view, reading the queue now and each time `woken` says.
    // What the launch hands the window, each needed from its first frame.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        backend: Backend,
        woken: dm_desktop_base::wakes::Woken,
        requests: Option<std::sync::mpsc::Receiver<dm_desktop_base::control::Request>>,
        assistants_only: bool,
        show_asked: std::sync::Arc<std::sync::atomic::AtomicBool>,
        hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Downloads {
        let last_limit = dm_shell::app::commands::remembered_limit(&backend.settings());
        let focus = cx.focus_handle();
        let list = state::ListView::new(window, cx);
        let windows = state::OpenWindows::new(
            window.window_bounds(),
            window.display(cx).map(|d| u64::from(d.id())),
        );
        let mut this = Downloads {
            backend,
            session: Session::default(),
            rows: Vec::new(),
            screen: Screen::default(),
            focus,
            // Limit speed turns back on at the limit last in force, kept across a restart:
            // from 0 it came back on at 1 KB/s.
            last_limit,
            list,
            known: state::Known {
                // The first read works it all out: an empty list's queues too.
                read_all: true,
                ..Default::default()
            },
            questions: state::Questions::new(window.window_handle()),
            windows,
            menus: Default::default(),
            layout: state::Layout {
                show_sidebar: true,
                show_toolbar: true,
                contrast: dm_desktop_base::contrast::system(),
                ..Default::default()
            },
            status: Default::default(),
            system: state::System {
                hidden,
                ..Default::default()
            },
            port: state::Port {
                requests,
                assistants_only,
                show_asked,
                ..Default::default()
            },
            threads: Default::default(),
        };
        // The sort the person left the list in.
        let settings = this.backend.settings();
        this.list.sort = dm_desktop_base::columns::kept_sort(&settings);
        // And the side panel and toolbar as they were left (gaps row U29).
        this.layout.show_sidebar = settings.show_categories;
        this.layout.show_toolbar = settings.show_toolbar;
        // The taskbar button's jump list (Windows): Add URL and the folder.
        dm_desktop_base::jumplist::publish(
            dm_desktop_base::i18n::t("Add URL"),
            dm_desktop_base::i18n::t("Open downloads folder"),
            &settings.download_dir,
        );
        // The clipboard watched, and the look asked for, as Options says.
        this.watch_clipboard(settings.watch_clipboard);
        crate::window::theme::follow(&settings.theme, cx);
        window.focus(&this.focus, cx);
        // The tray, and the close button that keeps dm in it.
        this.start_tray(window, cx);
        // A new install's first window, once.
        this.welcome_if_new(cx);
        // A link dragged from a browser is taken, as files are.
        crate::platform::drops::take_links(window);
        let me = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            me.update(cx, |view, cx| view.close_asked(window, cx))
                .unwrap_or(true)
        });
        crate::window::screen::Screens::register(
            window.window_handle(),
            this.screen.clone(),
            false,
            cx,
        );
        // A finished row's file is looked for off this thread from now on
        // (`look_for_files`), not asked of the disk as it is drawn.
        dm_desktop_base::rows::set_looked(Vec::new());
        this.read();
        // One task on the window's thread: each wake is one read, a burst of
        // queue changes one read a quarter of a second later.
        // Redrawn in the system's colours when the person switches between
        // light and dark.
        // The list narrowed as one types in the top bar's search box;
        // Return there chooses the first row it leaves.
        cx.subscribe_in(
            &this.list.filter,
            window,
            |this: &mut Downloads, field, event, window, cx| match event {
                gpui_component::input::InputEvent::Change => {
                    this.list.filter_text = field.read(cx).value().to_string();
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
                // Back from a file manager, a file deleted there is said.
                this.known.files_looked = None;
                this.look_for_files(cx);
            }
        })
        .detach();
        // A login launch into the tray: GPUI's X11 windows are shown however
        // they were asked to open, so it is put away there once its first
        // frame is drawn (GPUI has mapped it on its own connection by then).
        if hidden && cfg!(all(unix, not(target_os = "macos"))) {
            window.on_next_frame(|window, _| crate::platform::syswindow::hide(window));
        }
        // A login launch waits in the tray; where nothing is showing a tray
        // a while later (a panel that never came), its window opens rather
        // than dm running unseen.
        if hidden {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(HIDDEN_WAIT).await;
                let _ = this.update(cx, |this, cx| {
                    if this.system.tray.is_none() || !dm_desktop_base::tray::host_showing() {
                        this.show_main(cx);
                    }
                });
            })
            .detach();
        }
        // Its frame as it moves and is resized, kept as dm quits.
        cx.observe_window_bounds(window, |this, window, cx| {
            this.windows.frame = window.window_bounds();
            this.windows.display = window.display(cx).map(|d| u64::from(d.id()));
        })
        .detach();
        // GPUI ending dm without Exit: on Windows, the session ending (a
        // logoff, a shutdown or restart) quits GPUI from its own window, which
        // ends the process as soon as this returns. The downloads are stopped
        // and awaited here first, as Exit stops them (gaps row 186).
        cx.on_app_quit(|this, _| {
            if !this.questions.quit_asked {
                this.wind_down();
                this.questions.quit_asked = true;
            }
            async {}
        })
        .detach();
        cx.observe_window_appearance(window, |_, window, cx| {
            // The text fields and menus (gpui-component's) follow too.
            gpui_component::Theme::sync_system_appearance(Some(window), cx);
            cx.notify();
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            // When the window last looked: its clock runs from there.
            let mut looked = Instant::now();
            loop {
                // Nothing tells the window of a running download's progress
                // or of a clock (the schedules, When done's countdown): it
                // looks of its own accord (`clock_every`).
                let every = this
                    .read_with(cx, |this, _| this.clock_every())
                    .unwrap_or(IDLE);
                let timer = cx
                    .background_executor()
                    .timer((looked + every).saturating_duration_since(Instant::now()));
                let wake = match woke(&woken, timer).await {
                    Woke::By(wake) => wake,
                    Woke::Clock => Wake::Now,
                    Woke::Gone => return,
                };
                // A change to the queue is read a quarter of a second later,
                // the burst within it as one; but something to act on now (a
                // browser's hand-off, another dm's order) ends that wait:
                // behind a running download's
                // changes, a hand-off waited the quarter out before its
                // download began.
                if wake == Wake::Soon {
                    // No later than the clock's own look, which reads it.
                    let until = (Instant::now() + SOON).min(looked + every);
                    loop {
                        let timer = cx
                            .background_executor()
                            .timer(until.saturating_duration_since(Instant::now()));
                        match woke(&woken, timer).await {
                            Woke::By(Wake::Soon) => {}
                            Woke::By(Wake::Now) | Woke::Clock | Woke::Gone => break,
                        }
                    }
                }
                while woken.try_recv().is_ok() {}
                // Asked to end (a logout, `kill`, an uninstall): as Exit does,
                // the queue written down first.
                if dm_desktop_base::signals::SIGNALLED.load(Ordering::SeqCst) {
                    let _ = in_own_window(&this, cx, |this, _, cx| this.quit(cx));
                    return;
                }
                let done = in_own_window(&this, cx, |this, window, cx| this.wake(window, cx));
                if done.is_none() {
                    return;
                }
                looked = Instant::now();
            }
        })
        .detach();
        this
    }

    /// One wake: the queue read, the tick's work, the control port
    /// answered, and the window drawn again only where something shown may
    /// have changed: a wake that changed nothing drew the whole window for
    /// nothing (gaps row 181).
    pub(crate) fn wake(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let beside = self.shown_beside();
        let changed = self.read();
        self.taskbar_follow(window);
        self.tick(cx);
        self.watch_asks(cx);
        let answered = self.answer_requests(window, cx);
        if needs_drawing(
            changed,
            answered,
            self.moves_with_time(),
            beside != self.shown_beside(),
        ) {
            cx.notify();
        }
        self.look_for_files(cx);
    }

    /// How far the downloads are, on the taskbar button, so a minimized
    /// window still says it: green while they
    /// run, yellow paused, red a minute after one failed (Windows); a bar
    /// and a count on the Dock (macOS) and the launcher (Linux).
    fn taskbar_follow(&mut self, window: &Window) {
        use dm_desktop_base::taskbar;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mode = taskbar::mode_of(&self.rows, now);
        #[cfg(windows)]
        if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) {
            self.system.taskbar.show(handle.as_raw(), mode);
        }
        #[cfg(not(windows))]
        let _ = window;
        self.system
            .taskbar
            .show_outside(taskbar::outside_of(mode, taskbar::running_of(&self.rows)));
    }

    /// How long the window sleeps when nothing wakes it: a quarter of a
    /// second while a download runs, whose figures nothing else tells it of
    /// (the engine reports them that often;
    /// read once a second, they moved in jumps); a second for When done's
    /// countdown and the clipboard; else 30 seconds.
    fn clock_every(&self) -> Duration {
        clock(
            self.rows.iter().any(|r| r.state == State::Running),
            self.session.countdown.is_some() || self.system.clipboard.on,
        )
    }

    /// Answers what the control port asked, in order: who may ask as the
    /// application says, the window's methods through its `WindowProbe`,
    /// the rest through the application (`control_api`).
    fn answer_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let asked: Vec<_> = match &self.port.requests {
            Some(requests) => requests.try_iter().collect(),
            None => return false,
        };
        if asked.is_empty() {
            return false;
        }
        let allow_mcp = self.backend.settings().allow_mcp;
        for request in asked {
            let answer = match dm_shell::app::control_api::refused(
                request.via_mcp,
                self.port.assistants_only,
                allow_mcp,
            ) {
                Some(why) => Err(why),
                None => crate::probe::answer(self, &request.method, &request.params, window, cx),
            };
            self.reply(&request.method, (request.reply, answer));
        }
        true
    }

    /// Carries out a command from the menu bar or a key. What the shell
    /// cannot do yet is greyed in the menus (`menus::carried_out`), and a
    /// key for it does nothing.
    pub fn run(&mut self, command: dm_shell::app::commands::Command, cx: &mut Context<Self>) {
        self.carry_out(command, cx);
        // The menus' ticks follow what it changed: When done, the speed
        // limiter, the View menu's.
        self.refresh_menus(cx);
    }

    fn carry_out(&mut self, command: dm_shell::app::commands::Command, cx: &mut Context<Self>) {
        use dm_shell::app::commands::Command;
        if !self.enabled(command) {
            return;
        }
        // Held by Options (B42): Quit and the dialogs' own still go. An open
        // question (Linux, `holder`) holds the window's own clicks and keys
        // only, not what the tray, `--when-done` or the control port asks.
        let options_open = self
            .windows
            .options_window
            .is_some_and(|w| cx.windows().contains(&w));
        if command != Command::Quit
            && command != Command::Options
            && options_open
            && self.bring_holder(cx)
        {
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
                self.layout.show_sidebar = !self.layout.show_sidebar;
                self.keep_shown(cx);
                cx.notify();
                return;
            }
            Command::ToggleToolbar => {
                self.layout.show_toolbar = !self.layout.show_toolbar;
                self.keep_shown(cx);
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
                self.sort_by(column, cx);
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
                    self.windows.progress_closed.remove(&id);
                    self.show_progress(id, cx);
                }
                return;
            }
            Command::SiteFiles => {
                crate::dialogs::grabber::open(cx.entity().downgrade(), cx);
                return;
            }
            Command::UpdateLink => {
                crate::dialogs::rowdialogs::update_link(cx.entity().downgrade(), chosen[0].id, cx);
                return;
            }
            Command::MoveToQueue => {
                let ids = chosen.iter().map(|r| r.id).collect();
                crate::dialogs::rowdialogs::move_to_queue(cx.entity().downgrade(), ids, cx);
                return;
            }
            Command::MoveRename => {
                crate::dialogs::rowdialogs::move_rename(cx.entity().downgrade(), chosen, cx);
                return;
            }
            Command::Properties => {
                for row in &chosen {
                    crate::dialogs::properties::open(cx.entity().downgrade(), row.id, cx);
                }
                return;
            }
            Command::Scheduler => {
                crate::dialogs::scheduler::open(cx.entity().downgrade(), self.current_queue(), cx);
                return;
            }
            Command::SpeedLimiterSettings => {
                crate::dialogs::limiter::open(cx.entity().downgrade(), cx);
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
            // Downloads › When done: what dm does once the queue has run
            // dry, for this session.
            Command::WhenDoneNothing
            | Command::WhenDoneQuit
            | Command::WhenDoneSleep
            | Command::WhenDoneHibernate
            | Command::WhenDoneShutDown => {
                if let Some(chosen) = dm_shell::app::commands::when_done(command) {
                    self.session.when_done = chosen;
                }
                cx.notify();
                return;
            }
            Command::AddUrl => {
                crate::dialogs::addurl::open(cx.entity().downgrade(), cx);
                return;
            }
            Command::AddSeries => {
                crate::dialogs::series::open(cx.entity().downgrade(), cx);
                return;
            }
            Command::AddFromClipboard => {
                self.add_from_clipboard(cx);
                return;
            }
            Command::Options => {
                self.open_options(cx);
                return;
            }
            // View › Drop target: kept in Options, and the card shown or put
            // away.
            Command::ToggleDropTarget => {
                let mut wanted = self.backend.settings();
                wanted.drop_target = !wanted.drop_target;
                if let Err(e) = self.apply_settings(wanted, cx) {
                    self.say(e);
                }
                self.follow_drop_target(cx);
                cx.notify();
                return;
            }
            Command::About | Command::CheckForUpdates => {
                self.open_about(command == Command::CheckForUpdates, cx);
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
        if dm_shell::app::commands::is_verb(command) {
            // Undo stands until the next command that changes the list (gaps row U14).
            self.status.undo.forget();
            // What Delete Completed takes off, kept for its Undo (B24).
            let kept = (command == Command::DeleteCompleted).then(|| self.backend.clearable_rows());
            // Off the window's thread: Stop on ten thousand chosen rows is
            // ten thousand changes to the queue (gaps row 183).
            let queue = self.current_queue();
            let ids: Vec<dm_core::Id> = chosen.iter().map(|r| r.id).collect();
            self.later(
                move |backend| dm_shell::app::commands::verb(command, backend, &chosen, &queue),
                move |this, ran, cx| this.verb_done(command, &ids, ran, kept, cx),
                cx,
            );
            return;
        }
        self.read();
        cx.notify();
    }

    /// What a command `commands::verb` carried out on the rows `chosen`
    /// did, said and shown, once it is done.
    fn verb_done(
        &mut self,
        command: dm_shell::app::commands::Command,
        chosen: &[dm_core::Id],
        ran: Option<dm_shell::app::commands::Ran>,
        kept: Option<Vec<(usize, Entry)>>,
        cx: &mut Context<Self>,
    ) {
        use dm_shell::app::commands::Command;
        self.read();
        if let Some(dm_shell::app::commands::Ran::Cleared(n)) = ran {
            // Undo puts them back, as it does what Remove took off (B24, gaps row U14).
            if n > 0 && self.status.undo.keep(kept.unwrap_or_default()) > 0 {
                self.undo_expires(cx);
            }
            self.say(dm_desktop_base::i18n::tn(
                "Deleted {} completed or cancelled download from the list.||Deleted {} completed or cancelled downloads from the list.",
                n as u64,
                &[&n],
            ));
        }
        // Resume opens each row's progress window where Options says, but
        // not one its person closed: fifteen Resumes opened it fifteen
        // times over the list. Show progress still opens it.
        if command == Command::DownloadNow && self.backend.settings().show_progress {
            for id in chosen {
                if !self.windows.progress_closed.contains(id) {
                    self.show_progress(*id, cx);
                }
            }
        }
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
    ) -> Result<usize, String> {
        let later = with.later;
        let items = urls.into_iter().map(|u| (u, with.clone())).collect();
        let added = self.session.add(&self.backend, items);
        if let Some(id) = added.rows.first().map(|(id, _, _)| *id) {
            // The new row is the one chosen, as the person expects of a row
            // just added.
            self.list.selected.clear();
            self.list.selected.insert(id);
        }
        if let Some((failed, e)) = added.partly() {
            // The rest went in; the ones that did not are said, not lost.
            self.say(match failed {
                1 => dm_desktop_base::i18n::tf("One of the links was not added: {}", &[&e]),
                n => dm_desktop_base::i18n::tf("{} of the links were not added: {}", &[&n, &e]),
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
        added.outcome()
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
        let asked = crate::dialogs::question::Asked {
            title: ask.title.into(),
            lines: std::iter::once(ask.sentence)
                .chain(ask.why)
                .chain(std::iter::once(ask.question.to_string()))
                .map(Into::into)
                .collect(),
            named: None,
            detail: None,
            toggle: None,
            ticked: false,
            // Keep waiting beside Start now, the default, rightmost.
            buttons: vec![ask.start.into(), ask.wait.into()],
            default: 0,
            escape: 1,
        };
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, _, cx| {
            if button != 0 {
                return;
            }
            let _ = view.update(cx, move |view, cx| {
                // Off the window's thread, as the toolbar's Resume is.
                view.later_then_read(
                    move |backend| {
                        for id in &ids {
                            backend.start_outside_schedule(*id);
                        }
                    },
                    cx,
                );
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
        self.add_items_sized(items, &Default::default(), cx)
    }

    /// [`Self::add_items`], with the sizes a dialog already learned for some of the links,
    /// shown on their rows until each learns its own.
    pub fn add_items_sized(
        &mut self,
        items: Vec<(String, dm_core::AddOptions)>,
        sizes: &std::collections::HashMap<String, u64>,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let added = self.session.add(&self.backend, items);
        for (id, url, _) in &added.rows {
            if let Some(size) = sizes.get(url) {
                self.known.sizes_told.insert(*id, *size);
            }
        }
        if let Some((failed, e)) = added.partly() {
            // The rest went in; the ones that did not are said, not lost.
            self.say(match failed {
                1 => dm_desktop_base::i18n::tf("One of the links was not added: {}", &[&e]),
                n => dm_desktop_base::i18n::tf("{} of the links were not added: {}", &[&n, &e]),
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
            .questions
            .cancelled_hand_offs
            .cancelled(site, &settings.catch.skip_hosts)
        {
            return;
        }
        let asked = crate::dialogs::question::Asked {
            title: dm_desktop_base::i18n::t("Leave this site to the browser?").into(),
            lines: vec![dm_desktop_base::i18n::tf(
                "You cancelled two downloads from {}. Leave its downloads to the browser?",
                &[&site],
            )
            .into()],
            named: None,
            detail: None,
            toggle: None,
            ticked: false,
            buttons: vec![
                dm_desktop_base::i18n::t("Leave them to the browser").into(),
                dm_desktop_base::i18n::t("Keep handing them over").into(),
            ],
            default: 0,
            escape: 1,
        };
        let site = site.to_string();
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, _, cx| {
            if button != 0 {
                return;
            }
            let _ = view.update(cx, |view, _| {
                let mut wanted = view.backend.settings();
                if !wanted.catch.skip_hosts.contains(&site) {
                    wanted.catch.skip_hosts.push(site);
                }
                if let Err(e) = view.backend.set_settings(wanted) {
                    view.status.problems.push(e);
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
            self.questions.countdown_shown = false;
            return;
        }
        if self.questions.countdown_shown {
            return;
        }
        self.questions.countdown_shown = true;
        let Some(asked) = countdown_question(self) else {
            return;
        };
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, _, cx| {
            let _ = view.update(cx, |view, cx| {
                view.questions.countdown_shown = false;
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
        let live: crate::dialogs::question::Live = Box::new(countdown_question);
        self.sheet(asked, None, Some(live), answer, cx);
    }

    /// Clears the search box: every row of the place shown again.
    pub fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.list
            .filter
            .update(cx, |f, cx| f.set_value("", window, cx));
        self.list.filter_text.clear();
        self.reshow();
        cx.notify();
    }

    /// Find (⌘F): the keyboard to the top bar's search box, its words
    /// chosen, as a code editor's ⌘P puts it in its command centre.
    pub fn find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.list.filter.update(cx, |f, cx| {
            f.focus(window, cx);
            f.select_all(window, cx);
        });
    }

    /// Find next (`forward`) or previous: the next row the search box shows (it narrows by name,
    /// address or note), going round the list, chosen and brought into view (gaps row U30, B48).
    pub fn find_step(&mut self, forward: bool) {
        use dm_desktop_base::i18n::tf;
        if self.list.filter_text.trim().is_empty() {
            return;
        }
        let from = self
            .list
            .shown
            .iter()
            .position(|&i| self.list.selected.contains(&self.rows[i].id));
        match dm_desktop_base::find::step(self.list.shown.len(), from, forward) {
            Some(at) => {
                let row = &self.rows[self.list.shown[at]];
                let name = dm_shell::app::names::name_of(row);
                self.list.selected = std::collections::BTreeSet::from([row.id]);
                self.list
                    .scroll
                    .scroll_to_item(at, gpui::ScrollStrategy::Center);
                self.say(tf("Found {}.", &[&name]));
            }
            None => self.say(tf("Nothing is called “{}”.", &[&self.list.filter_text])),
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
        // Off the window's thread, as the commands on rows are (gaps row
        // 183), and said once done.
        let asked = queue.to_string();
        let queue = queue.to_string();
        self.later(
            move |backend| dm_shell::app::commands::verb(command, backend, &[], &asked),
            move |this, ran, cx| this.ran_on_queue(command, &queue, ran, cx),
            cx,
        );
    }

    /// What Start Queue or Stop Queue on `queue` did, said.
    fn ran_on_queue(
        &mut self,
        command: dm_shell::app::commands::Command,
        queue: &str,
        ran: Option<dm_shell::app::commands::Ran>,
        cx: &mut Context<Self>,
    ) {
        use dm_desktop_base::i18n::{tf, tn};
        use dm_shell::app::commands::{Command, Ran};
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
                self.status.said_schedule_wait = n > 0 && !lets;
                if queue == dm_core::RECHECK_QUEUE {
                    dm_shell::app::commands::recheck(&self.backend, self.threads.rechecked.clone());
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
        let settings = self.backend.settings();
        let ticks = crate::window::menus::Ticks {
            queues: self.known.queue_list.clone(),
            when_done: self.session.when_done,
            speed_limit: settings.speed_limit,
            sidebar: self.layout.show_sidebar,
            toolbar: self.layout.show_toolbar,
            drop_target: settings.drop_target,
            column_order: settings.column_order.clone(),
            columns_hidden: settings.columns_hidden.clone(),
            toolbar_style: settings.toolbar_style.clone(),
            toolbar_order: settings.toolbar_order.clone(),
            toolbar_hidden: settings.toolbar_hidden.clone(),
            text_size: settings.text_size,
            language: settings.language.clone(),
            sorted_by: self.list.sort.and_then(|(column, _)| sort_command(column)),
            resume: dm_desktop_base::rows::resume_word(&self.chosen()),
            off: self.commands_off(),
        };
        if self.menus.menu_ticks.as_ref() != Some(&ticks) {
            crate::window::menus::set(cx, &ticks);
            self.menus.menu_ticks = Some(ticks);
        }
    }

    /// The Recheck queue's counts, once its check is done.
    pub(crate) fn say_rechecked(&mut self) {
        let done = dm_core::lock(&self.threads.rechecked).take();
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
        let results = self.threads.search_results.clone();
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
        let done: Vec<_> = std::mem::take(&mut *dm_core::lock(&self.threads.search_results));
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
                            Err(e) => self.status.problems.push(tf(
                                "the search {} found files that could not be added: {}",
                                &[&name, &e],
                            )),
                        }
                    }
                    let settings = self.backend.settings();
                    if let Some(wanted) =
                        dm_shell::app::search::seen_saved(&settings, &name, remember)
                    {
                        if let Err(e) = self.backend.set_settings(wanted) {
                            self.status.problems.push(tf(
                                "what the search {} has seen could not be saved: {}",
                                &[&name, &e],
                            ));
                        }
                    }
                }
                Err(e) => self
                    .status
                    .problems
                    .push(tf("the search {} failed: {}", &[&name, &e])),
            }
        }
        let now = std::time::Instant::now();
        if self
            .threads
            .searches_checked
            .is_some_and(|at| now.duration_since(at) < SEARCHES_EVERY)
        {
            return;
        }
        self.threads.searches_checked = Some(now);
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
            self.status.problems.push(dm_desktop_base::i18n::tf(
                "when the saved searches last ran could not be saved: {}",
                &[&e],
            ));
        }
        for search in due {
            self.run_search(search);
        }
    }

    /// The progress window of `id`, opened (or brought forward).
    pub fn show_progress(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        if let Some(handle) = self.windows.progress_windows.get(&id) {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        let nth = self.windows.progress_windows.len();
        let me = cx.entity().downgrade();
        let main = self.questions.main;
        cx.defer(move |cx| {
            if let Some(handle) = crate::dialogs::progress::open(me.clone(), id, nth, main, cx) {
                let _ = me.update(cx, |view, _| {
                    view.windows.progress_windows.insert(id, handle);
                });
            }
        });
    }

    /// Where an opener says what went wrong, from its thread.
    pub(crate) fn opener_said(&self) -> std::sync::Arc<std::sync::Mutex<Option<String>>> {
        self.threads.opener_said.clone()
    }

    /// The row of download `moved` put where the row of `onto` stands, in the queue's order
    /// (gaps row U39): what a row dragged onto another does while the list is in its own order.
    pub(crate) fn move_row(&mut self, moved: dm_core::Id, onto: dm_core::Id) {
        if moved == onto || self.list.sort.is_some() {
            return;
        }
        let at = |id| self.rows.iter().position(|r| r.id == id);
        if let (Some(from), Some(to)) = (at(moved), at(onto)) {
            self.backend.shift(moved, to as isize - from as isize);
            self.read();
        }
    }

    /// Runs `command` on the row of download `id` alone, as its own menu would: a step a
    /// window over it offers (gaps row U10).
    pub(crate) fn run_on(
        &mut self,
        id: dm_core::Id,
        command: dm_shell::app::commands::Command,
        cx: &mut Context<Self>,
    ) {
        self.list.selected = [id].into();
        self.list.anchor = Some(id);
        self.run(command, cx);
    }

    /// Opens a file with its usual app, off the window's thread.
    pub fn open_path(&self, path: &str) {
        let said = self.threads.opener_said.clone();
        self.backend
            .platform
            .open_later(path, Box::new(move |why| *dm_core::lock(&said) = Some(why)));
    }

    /// Shows a file in the file manager, off the window's thread.
    pub fn reveal_path(&self, path: &str) {
        let said = self.threads.opener_said.clone();
        self.backend
            .platform
            .reveal_later(path, Box::new(move |why| *dm_core::lock(&said) = Some(why)));
    }

    /// Open with…: the app chosen in the system's own panel, the window
    /// going on meanwhile; then `after`, chosen or not.
    pub fn open_path_with(
        &mut self,
        path: &str,
        after: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let chosen = crate::platform::services::ask(crate::platform::services::Asked::app());
        let path = path.to_string();
        cx.spawn(async move |this, cx| {
            let app = chosen.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(app) = app {
                    if let Err(e) = this.backend.platform.open_with(&path, &app) {
                        this.say(e);
                    }
                }
                after(this, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// What an opener said went wrong, from its thread.
    pub(crate) fn say_opener(&mut self) {
        let said = dm_core::lock(&self.threads.opener_said).take();
        if let Some(why) = said {
            self.say(why);
        }
    }

    /// The name the list shows for `row`: one not yet placed reads as the
    /// name it will get ("big (1).zip", not "big.zip").
    pub(crate) fn name_shown(&self, row: &Entry) -> String {
        let settings = self.backend.settings();
        dm_shell::app::names::name_it_will_get_after(
            &self.rows,
            row,
            &settings.download_dir,
            &mut settings.categories(),
            None,
        )
    }

    /// Whether a dialog stands in front of the list: a question, or a
    /// dialog's window. Progress windows and "Download complete" stand
    /// beside it, not in front.
    pub(crate) fn dialog_in_front(&self, cx: &gpui::App) -> bool {
        self.questions.sheet_open
            || crate::window::screen::Screens::all(cx)
                .iter()
                .any(|(w, _, dialog)| {
                    *dialog
                        && !self.windows.progress_windows.values().any(|p| p == w)
                        && self.windows.complete_window.as_ref() != Some(w)
                })
    }

    /// Row `id`'s File properties window, where it is still open.
    pub(crate) fn properties_window(
        &self,
        id: dm_core::Id,
        cx: &gpui::App,
    ) -> Option<gpui::AnyWindowHandle> {
        let open = cx.windows();
        self.windows
            .properties_windows
            .get(&id)
            .filter(|w| open.contains(w))
            .copied()
    }

    /// How many File properties windows are open.
    pub(crate) fn properties_open(&self, cx: &gpui::App) -> usize {
        let open = cx.windows();
        self.windows
            .properties_windows
            .values()
            .filter(|w| open.contains(w))
            .count()
    }

    /// Row `id`'s File properties window opened; those since closed are
    /// forgotten.
    pub(crate) fn properties_opened(&mut self, id: dm_core::Id, window: gpui::AnyWindowHandle) {
        self.windows.properties_windows.insert(id, window);
    }

    /// The dialog that keeps the clicks to itself and so holds this window
    /// (B42): Options, while open; on Linux an open question too, which GPUI
    /// holds a parent for only as a `Dialog`, which a question is not there
    /// (`question::KIND`). Elsewhere a question is a sheet or a `Dialog`, and
    /// the system holds the window for it.
    pub(crate) fn holder(&self, cx: &gpui::App) -> Option<gpui::AnyWindowHandle> {
        let open = cx.windows();
        let question = self
            .questions
            .question_window
            .filter(|_| cfg!(not(any(target_os = "macos", windows))));
        [self.windows.options_window, question]
            .into_iter()
            .flatten()
            .find(|w| open.contains(w))
    }

    /// Where a dialog holds the window, it comes forward; true if one does.
    pub(crate) fn bring_holder(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(holder) = self.holder(cx) else {
            return false;
        };
        cx.defer(move |cx| {
            let _ = holder.update(cx, |_, window, _| window.activate_window());
        });
        true
    }

    /// The main window.
    pub(crate) fn main_window(&self) -> gpui::AnyWindowHandle {
        self.questions.main
    }

    /// The main window back in front, from the tray or wherever it is, after
    /// the update that asked.
    pub(crate) fn show_main(&mut self, cx: &mut Context<Self>) {
        self.system.hidden = false;
        let main = self.questions.main;
        cx.defer(move |cx| {
            let _ = main.update(cx, |_, window, _| {
                crate::platform::syswindow::show(window);
                window.activate_window();
            });
            // A dialog open over it stays in front, as a person left it:
            // brought forward after the main window, not buried under it.
            if let Some((dialog, _, _)) = crate::window::screen::Screens::all(cx)
                .into_iter()
                .find(|(_, _, dialog)| *dialog)
            {
                let _ = dialog.update(cx, |_, window, _| window.activate_window());
            }
            cx.activate(true);
        });
    }

    /// Whether a second launch, the browser or the Dock asked for the
    /// window since the last look.
    pub(crate) fn take_show_asked(&self) -> bool {
        self.port
            .show_asked
            .swap(false, std::sync::atomic::Ordering::Relaxed)
    }

    /// The rows chosen, in the list's order.
    pub fn chosen(&self) -> Vec<&Entry> {
        // From the choice, not from every row shown: the toolbar asks for
        // each of its buttons, a dozen times a frame (gaps row 180).
        let mut chosen: Vec<(usize, &Entry)> = self
            .list
            .selected
            .iter()
            .filter_map(|id| {
                let at = *self.list.positions.get(id)?;
                let row = self.list.shown.get(at).and_then(|&r| self.rows.get(r))?;
                (row.id == *id).then_some((at, &**row))
            })
            .collect();
        chosen.sort_unstable_by_key(|(at, _)| *at);
        chosen.into_iter().map(|(_, row)| row).collect()
    }

    /// The queue Start and Stop Queue act on: the one shown, else the Main
    /// queue.
    pub fn current_queue(&self) -> String {
        match &self.list.place {
            Place::Queue(name) => name.clone(),
            _ => dm_core::MAIN_QUEUE.to_string(),
        }
    }

    /// One read of the application: the rows and what follows from them.
    /// Says whether anything the window shows may have changed
    /// (`read_rows`).
    pub(crate) fn read(&mut self) -> bool {
        let first = crate::port::FRAMES.load(Ordering::Relaxed) == 0;
        if first {
            dm_core::finish_trace::mark("start: a read before the first frame begins");
        }
        let changed = self.read_rows();
        if first {
            dm_core::finish_trace::mark("start: a read before the first frame ends");
        }
        changed
    }

    /// Shows the rows of `place`.
    pub fn choose(&mut self, place: Place) {
        self.list.place = place;
        self.reshow();
    }

    /// A double-click on the row at `position`: what Options › General
    /// says a double-click does, Properties by default, for any download;
    /// Open or its folder for a finished one. A download that is running,
    /// waiting or stopped shows its progress window instead, which is what
    /// a person looking at one still going wants to see.
    pub fn double_click(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        use dm_shell::app::commands::Command;
        let Some(row) = self
            .position_of(id)
            .and_then(|at| self.list.shown.get(at))
            .and_then(|&r| self.rows.get(r))
        else {
            return;
        };
        let finished = row.state == State::Completed;
        let going = matches!(row.state, State::Running | State::Queued | State::Paused);
        let id = row.id;
        let command = match self.backend.settings().double_click.as_str() {
            "open" if finished => Command::OpenFile,
            "folder" if finished => Command::OpenFolder,
            _ if going => Command::ShowProgress,
            _ => Command::Properties,
        };
        self.list.selected = [id].into();
        self.list.anchor = Some(id);
        self.run(command, cx);
    }

    /// The row of download `id`, found by where the list shows it, else
    /// by a look over every row (a row the place chosen does not show).
    pub(crate) fn row_of(&self, id: dm_core::Id) -> Option<&Arc<Entry>> {
        self.list
            .positions
            .get(&id)
            .and_then(|&at| self.list.shown.get(at))
            .and_then(|&r| self.rows.get(r))
            .filter(|r| r.id == id)
            .or_else(|| self.rows.iter().find(|r| r.id == id))
    }

    /// Says `sentence` in the status bar.
    pub fn say(&mut self, sentence: String) {
        self.status.said = Some((sentence, std::time::Instant::now()));
        self.status.said_schedule_wait = false;
    }
}

impl Render for Downloads {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.known.renders += 1;
        if crate::port::FRAMES.fetch_add(1, Ordering::Relaxed) == 0 {
            dm_core::finish_trace::mark("start: first frame drawn");
        }
        // Its waits, copies and disk calls, counted in a debug build (row 180).
        let _drawing = dm_shell::app::costs::drawing();
        if !std::mem::take(&mut self.known.drawn_fresh) {
            self.known.drawn = std::rc::Rc::new(self.backend.settings());
        }
        // Resume's words in the menu bar follow the rows chosen (Retry,
        // Start now), however they were chosen: built again after this
        // frame where they changed.
        // And what is greyed follows them, and the rows' states (gaps row U5).
        let resume = dm_desktop_base::rows::resume_word(&self.chosen());
        let off = self.commands_off();
        if self
            .menus
            .menu_ticks
            .as_ref()
            .is_some_and(|t| t.resume != resume || t.off != off)
        {
            let me = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = me.update(cx, |this, cx| this.refresh_menus(cx));
            });
        }
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let update_bar = self.update_bar(theme, &screen, cx);
        let holding = self.holder(cx).is_some();
        let browser_bar = self.browser_bar(theme, &screen, cx);
        let shown = self.shown_bars(update_bar, browser_bar, theme, &screen, cx);
        let root = div()
            .id("downloads")
            .role(Role::Application)
            .aria_label("dm")
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus);
        let root = Self::list_actions(root, cx);
        Self::list_keys(root, cx)
            // Clear under Mica (the toolbar on it too); the list's column
            // below stays solid.
            .bg(crate::window::theme::frame(theme.background))
            .text_color(theme.text)
            // The system's size for a list's rows and every control.
            .text_size(crate::window::zoom::px(crate::widgets::native::TEXT))
            .child(crate::window::titlebar::titlebar(
                self, theme, &screen, window, cx,
            ))
            .when(self.layout.show_toolbar, |root| {
                root.child(crate::window::toolbar::toolbar(
                    self, theme, &screen, window, cx,
                ))
            })
            .child(self.body(theme, &screen, shown, window, cx))
            .child(crate::window::statusbar::statusbar(
                self, theme, &screen, cx,
            ))
            .when_some(self.menus.menu, |root, at| {
                root.child(crate::window::rowmenu::menu(self, at, theme, &screen, cx))
            })
            .when_some(self.menus.place_menu.clone(), |root, (place, at)| {
                root.child(crate::window::placemenu::menu(
                    &place, at, theme, &screen, cx,
                ))
            })
            // Held by Options, the window takes no click: one brings Options
            // forward (B42).
            .when(holding, |root| {
                root.child(
                    div()
                        .id("held")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .occlude()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.bring_holder(cx);
                            }),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{clock, IDLE, RUNNING};
    use std::time::Duration;

    /// A running download's figures are drawn four times a second, as the
    /// engine reports them; a clock once a second; nothing moving, rarely.
    #[test]
    fn a_running_download_is_drawn_four_times_a_second() {
        assert_eq!(RUNNING, Duration::from_millis(250));
        assert_eq!(clock(true, false), RUNNING);
        assert_eq!(clock(true, true), RUNNING);
        assert_eq!(clock(false, true), Duration::from_secs(1));
        assert_eq!(clock(false, false), IDLE);
    }
}
