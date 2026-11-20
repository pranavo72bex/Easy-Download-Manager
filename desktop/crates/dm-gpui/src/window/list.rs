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

mod adding;
mod carry;
mod commands;
mod complete;
mod following;
mod in_front;
mod keys;
mod later;
mod on_queue;
mod opening;
mod questions;
mod refresh;
mod removal;
mod rowline;
mod same_file;
mod sections;
mod sorting;
mod state;

use questions::countdown_question;
use refresh::clicked;
use refresh::needs_drawing;
pub(crate) use rowline::{kind_picture, line};
pub(crate) use sections::Shown;
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
        // A window asks about the same file as one already here before its body is fetched
        // (gaps row W18); without one, nobody is asked and it is fetched as before.
        this.backend.queue.ask_about_same_files(true);
        opening::first_settings(&mut this, window, cx);
        opening::listen(&this, hidden, window, cx);
        opening::read_on_wakes(woken, window, cx);
        this
    }

    /// One wake: the queue read, the tick's work, the control port
    /// answered, and the window drawn again only where something shown may
    /// have changed: a wake that changed nothing drew the whole window for
    /// nothing (gaps row 181).
    pub(crate) fn wake(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let beside = self.shown_beside();
        let changed = self.read();
        self.taskbar_follow(window, cx);
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
    /// and a count on the Dock (macOS) and the launcher (Linux); and in the
    /// window's title everywhere, "37% · 4.1 MB/s — dm" (gaps row U16).
    fn taskbar_follow(&mut self, window: &mut Window, cx: &mut gpui::App) {
        use dm_desktop_base::taskbar;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mode = taskbar::mode_of(&self.rows, now);
        #[cfg(windows)]
        if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(&*window) {
            self.system.taskbar.show(handle.as_raw(), mode);
        }
        self.system
            .taskbar
            .show_outside(taskbar::outside_of(mode, taskbar::running_of(&self.rows)));
        // Set only where it changed (`set_title`), and kept for the port.
        let title = taskbar::window_title(mode, taskbar::rate_of(&self.rows));
        crate::window::screen::set_title(window, &title, cx);
    }

    /// How long the window sleeps when nothing wakes it: a quarter of a
    /// second while a download runs, whose figures nothing else tells it of
    /// (the engine reports them that often;
    /// read once a second, they moved in jumps); a second for When done's
    /// countdown and the clipboard, and while a metered connection or a low battery holds
    /// the downloads, which go on within moments of its end (gaps row W11); else 30 seconds.
    fn clock_every(&self) -> Duration {
        clock(
            self.rows.iter().any(|r| r.state == State::Running),
            self.session.countdown.is_some()
                || self.system.clipboard.on
                || self.backend.held_by().is_some(),
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
        let allow_mcp = self.backend.settings().general.allow_mcp;
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
        // At a phone's width the box is opened across the top bar (gaps row N7).
        if self.layout.narrow.laid_out {
            self.layout.narrow.searching = true;
        }
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

    /// The menu bar's queue submenus, built again when the queues change.
    pub(crate) fn refresh_menus(&mut self, cx: &mut Context<Self>) {
        let settings = self.backend.settings();
        let ticks = crate::window::menus::Ticks {
            queues: self.known.queue_list.clone(),
            when_done: self.session.when_done,
            speed_limit: settings.advanced.speed_limit,
            sidebar: self.layout.show_sidebar,
            toolbar: self.layout.show_toolbar,
            drop_target: settings.drop_target,
            column_order: settings.column_order.clone(),
            columns_hidden: settings.columns_hidden.clone(),
            toolbar_style: settings.toolbar_style.clone(),
            toolbar_order: settings.toolbar_order.clone(),
            toolbar_hidden: settings.toolbar_hidden.clone(),
            text_size: settings.text_size,
            language: settings.general.language.clone(),
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

    /// The name the list shows for `row`: one not yet placed reads as the
    /// name it will get ("big (1).zip", not "big.zip").
    pub(crate) fn name_shown(&self, row: &Entry) -> String {
        let settings = self.backend.settings();
        dm_shell::app::names::name_it_will_get_after(
            &self.rows,
            row,
            &settings.folders.download_dir,
            &mut settings.categories(),
            None,
        )
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
        let command = match self.backend.settings().general.double_click.as_str() {
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
        let root = Self::list_keys(root, cx)
            // Clear under Mica (the toolbar on it too); the list's column
            // below stays solid.
            .bg(crate::window::theme::frame(theme.background))
            .text_color(theme.text)
            // The system's size for a list's rows and every control.
            .text_size(crate::window::zoom::px(crate::widgets::native::TEXT));
        // At a phone's width, the phone's layout of the same views (gaps row N7).
        let narrow = crate::window::narrow::is_narrow(window);
        self.layout.narrow.laid_out = narrow;
        let root = if narrow {
            self.narrow_window(root, theme, &screen, shown, window, cx)
        } else {
            root.child(crate::window::titlebar::titlebar(
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
        };
        root.when_some(self.menus.place_menu.clone(), |root, (place, at)| {
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
