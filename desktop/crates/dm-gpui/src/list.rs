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

/// The rows' height, in points.
pub(crate) const ROW_HEIGHT: f32 = 26.0;

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
    focus: FocusHandle,
    /// The place chosen in the side panel.
    pub(crate) place: Place,
    /// Each row's place and the places' counts (`dm_shell::app::places`).
    pub(crate) places: Places,
    /// The rows the list shows, in the place chosen: their indices in `rows`.
    pub(crate) shown: Vec<usize>,
    /// The column the list is sorted by, and whether up; none: as added.
    pub(crate) sort: Option<(Column, bool)>,
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
        };
        // The sort the person left the list in.
        let settings = this.backend.settings();
        this.sort = Column::from_key(&settings.sort_column).map(|c| (c, settings.sort_ascending));
        window.focus(&this.focus, cx);
        this.read();
        // One task on the window's thread: each wake is one read, a burst of
        // queue changes one read a quarter of a second later.
        // Redrawn in the system's colours when the person switches between
        // light and dark.
        cx.observe_window_appearance(window, |_, _, cx| cx.notify())
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
                let _ = this.update_in(cx, |this, _, cx| {
                    this.backend.flush();
                    cx.quit();
                });
                return;
            }
            if this
                .update_in(cx, |this, window, cx| {
                    this.read();
                    this.answer_requests(window, cx);
                    cx.notify();
                })
                .is_err()
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
        if !crate::menus::carried_out(command) {
            return;
        }
        if command == Command::Quit {
            self.backend.flush();
            cx.quit();
            return;
        }
        // The application's own verbs on the whole list need no selection.
        let _ = dm_shell::app::commands::verb(command, &self.backend, &[], "");
        self.read();
        cx.notify();
    }

    /// One read of the application: the rows and what follows from them.
    fn read(&mut self) {
        dm_shell::app::changes::take();
        let settings = self.backend.settings();
        let read = self.session.read(&self.backend, &settings);
        // What the read told beside the rows, which a row's words are said
        // from (ranges tried again, swarms, files taken by a later row, the
        // download limit), as the egui window sets it.
        dm_desktop_base::rows::set_retrying(read.snapshot.retrying);
        dm_desktop_base::rows::set_swarms(read.snapshot.swarms);
        dm_desktop_base::rows::set_limit_holds(read.snapshot.allowance_spent);
        self.rows = read.snapshot.rows;
        dm_desktop_base::rows::set_superseded(&self.rows);
        self.places.place_rows(&self.rows, &settings.categories());
        self.reshow();
    }

    /// Shows the rows of `place`.
    pub fn choose(&mut self, place: Place) {
        self.place = place;
        self.reshow();
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
            .filter(|&i| self.places.within(&self.place, &self.rows[i], &categories))
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
            .on_action(cx.listener(|this, run: &crate::menus::Run, _, cx| this.run(run.0, cx)))
            .bg(theme.background)
            .text_color(theme.text)
            .text_sm()
            .child(crate::toolbar::toolbar(self, theme, &screen, window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(crate::sidebar::sidebar(self, theme, &screen, window, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(crate::table::header(self, theme, &screen, cx))
                            .child(crate::table::rows(self, theme, cx)),
                    ),
            )
            .child(crate::statusbar::statusbar(self, theme, &screen))
    }
}
