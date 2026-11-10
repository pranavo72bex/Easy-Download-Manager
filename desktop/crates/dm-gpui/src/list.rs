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
        if !self.enabled(command) {
            return;
        }
        if command == Command::Quit {
            self.backend.flush();
            cx.quit();
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
        let _ = dm_shell::app::commands::verb(command, &self.backend, &chosen, &queue);
        self.read();
        cx.notify();
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
                toggle: ask.toggle.map(Into::into),
                go: ask.go.into(),
                keep: ask.keep.into(),
            }
        };
        let asked = words(false);
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |go, delete, cx| {
            if go {
                let _ = view.update(cx, |view, cx| {
                    view.removed(&ids, &files, delete);
                    cx.notify();
                });
            }
        });
        crate::question::ask(asked, Some(Box::new(words)), answer, cx);
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
            .on_action(cx.listener(|this, run: &crate::menus::Run, _, cx| this.run(run.0, cx)))
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
            .child(crate::statusbar::statusbar(self, theme, &screen, cx))
            .when_some(self.menu, |root, at| {
                root.child(crate::rowmenu::menu(self, at, theme, &screen, cx))
            })
    }
}
