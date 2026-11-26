// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A download's progress window (B53, B54, B63), one for each download
//! asked for, a window of its own that follows the list as it changes: its
//! title with how far it is first, its status (and under a failure one plain
//! next step), its size, what has come, its speed and time left, whether it
//! can resume, a bar (a piece for each connection where the pieces are
//! known), a limit of its own, its Connections (each piece's bar and its own
//! speed) or a torrent's swarm, and Stop, Retry or Resume, and Cancel.

use dm_core::{Id, State};
use dm_desktop_base::format;
use dm_desktop_base::i18n::{t, tf};
use dm_desktop_base::progress::{
    can_resume_words, piece_words, swarm_lines, title_with_percent, ConnRates,
};
use dm_shell::app::rules::GoOn;
use dm_shell::app::session::ThisDone;
use gpui::{
    div, point, prelude::*, size, App, Bounds, Context, Entity, Role, SharedString, WeakEntity,
    Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::widgets::native;
use crate::widgets::space;
use crate::window::a11y::Told;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

mod sections;
pub(crate) use sections::figure_words;

use sections::{connection_lines, facts, figure_line, networks_line, speed_line, whole_bar, Look};

/// The progress window's view.
/// The pop-up "When this download is done" (gaps row U36).
const THIS_DONE: &str = "this-done";

/// What "When this download is done" offers: what this computer can do.
fn this_done_choices() -> Vec<ThisDone> {
    ThisDone::ALL
        .into_iter()
        .filter(|choice| match choice {
            ThisDone::Then(action) => action.possible(dm_desktop_base::power::available),
            _ => true,
        })
        .collect()
}

pub struct Progress {
    view: WeakEntity<Downloads>,
    id: Id,
    rates: ConnRates,
    /// What the server said of resuming, kept while the parts are joined.
    resumable_seen: Option<bool>,
    /// Its pieces and whether it can resume, as last read from its plan
    /// and ledgers off the window's thread (`inspect`), and when: a frame
    /// draws what was read, and reads nothing itself (gaps row 180).
    pieces: Vec<dm_core::Connection>,
    resumable: Option<bool>,
    inspected: Option<std::time::Instant>,
    inspecting: bool,
    /// Connections is open.
    connections: bool,
    /// Which of the lines that open are open: the limit and When done (gaps row E9).
    opened: Opened,
    /// Its speed once a second over the last five minutes, while this window is open (gaps
    /// row W15).
    line: dm_desktop_base::statistics::SpeedLine,
    own_limit: Entity<InputState>,
    /// The window's keyboard: Escape closes it, as its close button does.
    focus: gpui::FocusHandle,
    screen: Screen,
}

/// How often an open progress window reads its download's pieces again.
const INSPECT_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

impl Progress {
    /// Reads the download's pieces and whether it can resume off the
    /// window's thread, at most every [`INSPECT_EVERY`], and draws again
    /// with them: each is a file read (its plan, a ledger a piece), and a
    /// frame that read them waited on the disk.
    fn inspect(&mut self, backend: &dm_shell::app::backend::Backend, cx: &mut Context<Self>) {
        if self.inspecting
            || self
                .inspected
                .is_some_and(|at| at.elapsed() < INSPECT_EVERY)
        {
            return;
        }
        self.inspecting = true;
        let id = self.id;
        let queue = backend.queue.clone();
        let read = backend.on_runtime(async move {
            let pieces = queue.connections(id).await;
            let resumable = queue.resumable(id).await;
            (pieces, resumable)
        });
        cx.spawn(async move |this, cx| {
            let read = read.await;
            let _ = this.update(cx, |this, cx| {
                this.inspecting = false;
                let now = std::time::Instant::now();
                this.inspected = Some(now);
                if let Some((pieces, resumable)) = read {
                    this.rates.sample(
                        now,
                        &pieces.iter().map(|c| (c.start, c.done)).collect::<Vec<_>>(),
                    );
                    this.pieces = pieces;
                    this.resumable = resumable;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Stop download, Retry, Resume (or Start now, outside the schedule).
    fn go(&mut self, stop: bool, cx: &mut Context<Self>) {
        let id = self.id;
        let _ = self.view.update(cx, |view, cx| {
            let Some(row) = view.rows.iter().find(|r| r.id == id).cloned() else {
                return;
            };
            // Off the window's thread, as the toolbar's Stop and Resume are;
            // the list is read again once it is done.
            if stop {
                view.later_then_read(move |backend| backend.pause(id), cx);
            } else {
                match dm_shell::app::rules::go_on(&row.state, row.paused_by_schedule) {
                    GoOn::Retry => view.later_then_read(move |backend| backend.retry(id), cx),
                    // Held by the schedule: asked whether to start it
                    // outside it.
                    GoOn::Held => {
                        view.ask_outside(vec![id], cx);
                        view.read();
                    }
                    GoOn::Resume | GoOn::Settled | GoOn::NotStopped => {
                        view.later_then_read(move |backend| backend.resume(id), cx)
                    }
                }
            }
            cx.notify();
        });
    }

    /// Cancel download: asked first.
    fn cancel(&mut self, cx: &mut Context<Self>) {
        let id = self.id;
        let _ = self.view.update(cx, |view, cx| view.ask_cancel(id, cx));
    }

    /// Open or Open folder on the finished download, and the window closed, as Download
    /// complete's buttons do.
    fn finished_with(
        &mut self,
        command: dm_shell::app::commands::Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.id;
        let _ = self
            .view
            .update(cx, |view, cx| view.run_on(id, command, cx));
        window.remove_window();
    }

    /// The step a failure names, taken (gaps row U10): another folder chosen and the download
    /// moved there, Update download link or Move/Rename on its row, the page it came from
    /// opened, or Options at its setting.
    fn take_step(&mut self, step: format::StepButton, cx: &mut Context<Self>) {
        use dm_shell::app::commands::Command;
        use format::StepButton;
        let id = self.id;
        let _ = self.view.update(cx, |view, cx| match step {
            StepButton::ChooseFolder => crate::platform::services::ask_then(
                crate::platform::services::Asked::folder(),
                cx,
                move |view: &mut Downloads, dir, cx| view.relocate_later(id, dir, cx),
            ),
            StepButton::UpdateLink => view.run_on(id, Command::UpdateLink, cx),
            StepButton::MoveRename => view.run_on(id, Command::MoveRename, cx),
            StepButton::OpenPage => {
                if let Some(row) = view.row_of(id) {
                    let page = if row.referrer.trim().is_empty() {
                        row.url.clone()
                    } else {
                        row.referrer.clone()
                    };
                    view.open_path(&page);
                }
            }
            StepButton::Options => {
                view.open_options_at(dm_desktop_base::options::Tab::Advanced, cx)
            }
        });
    }

    /// "When this download is done" set to the choice at `index` (`this_done_choices`).
    fn this_done(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(choice) = this_done_choices().get(index).copied() else {
            return;
        };
        let id = self.id;
        let _ = self.view.update(cx, |view, cx| {
            if choice == ThisDone::Nothing {
                view.session.this_done.remove(&id);
            } else {
                view.session.this_done.insert(id, choice);
            }
            cx.notify();
        });
        cx.notify();
    }

    /// "Limit this download to": on, at what is typed, or off.
    fn limit(&mut self, on: bool, cx: &mut Context<Self>) {
        let typed = self
            .own_limit
            .read(cx)
            .value()
            .trim()
            .parse::<u64>()
            .unwrap_or(500)
            .clamp(1, 10_000_000);
        let id = self.id;
        let _ = self.view.update(cx, |view, cx| {
            let kb_per_second = if on { typed } else { 0 };
            view.later_then_read(move |backend| backend.set_own_limit(id, kb_per_second), cx);
            cx.notify();
        });
    }
}

impl Render for Progress {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin_in(window);
        let screen = self.screen.clone();
        let Some(view) = self.view.upgrade() else {
            return div().into_any_element();
        };
        // The row by its download, and named as the list names it: a
        // search of every row and a name worked out over all of them, at
        // each frame, was what this window cost with a long list.
        let (row, backend, named) = {
            let view = view.read(cx);
            let Some(row) = view.row_of(self.id).cloned() else {
                // Its download taken out of the list (removed, here or by a script): the window
                // closes rather than stay open and empty. One the list has not read yet is
                // still in the queue, and waits.
                if view.backend.row(self.id).is_none() {
                    window.remove_window();
                }
                return div().into_any_element();
            };
            let named = view
                .known
                .shown_names
                .get(&self.id)
                .cloned()
                .unwrap_or_else(|| dm_shell::app::names::name_of(&row));
            (row, view.backend.clone(), named)
        };
        let id = self.id;
        self.inspect(&backend, cx);
        let connections = self.pieces.clone();
        let speeds: Vec<f64> = connections
            .iter()
            .map(|c| self.rates.rate(c.start))
            .collect();
        // While its parts are joined the plan they are read from is gone:
        // the last answer stands.
        let joining = row.state == State::Running && dm_shell::app::rules::fetched_all(&row);
        let resumable = match self.resumable {
            Some(answer) => {
                self.resumable_seen = Some(answer);
                Some(answer)
            }
            None if joining => self.resumable_seen,
            None => None,
        };
        // Finished, it is Download complete (gaps row U11, B63).
        let heading = if row.state == State::Completed {
            tf("Download complete — {}", &[&named])
        } else {
            title_with_percent(
                tf("Download progress — {}", &[&named]),
                row.progress.downloaded,
                row.progress.total,
            )
        };
        crate::window::screen::set_title(window, &heading, cx);
        // Known by that name to a script once finished, as the Download complete window was.
        if row.state == State::Completed {
            crate::window::screen::Screens::named(window.window_handle(), "Download complete", cx);
        }
        let p = &row.progress;
        // Its speed line, fed from the speed it shows (gaps row W15).
        let speed_now = if row.state == State::Running && !joining {
            p.speed
        } else {
            0.0
        };
        self.line.sample(std::time::Instant::now(), speed_now);
        let look = Look::new(theme, screen);
        let torrent = dm_core::is_torrent(&row.url);
        let pieces: Vec<(u64, u64)> = connections
            .iter()
            .map(|c| ((c.end - c.start).saturating_add(1), c.done))
            .collect();
        let multi = p.total.is_some() && connections.len() > 1;
        let this_done = view
            .read(cx)
            .session
            .this_done
            .get(&id)
            .copied()
            .unwrap_or_default();

        let conn_rows = (self.connections && !torrent)
            .then(|| connection_lines(&look, &row, &connections, &pieces, &speeds, joining));
        // Ranked (gaps row E9): how far, how fast and how long in one line of larger type
        // over the whole file's bar, then the facts.
        // Figures of one width each, so they do not jitter as they change (gaps row E33).
        let body = native::body(theme)
            .font_features(native::figures())
            .child(figure_line(&look, &row, joining))
            .child(whole_bar(&look, &row, &pieces, multi));
        let body = facts(&look, &row, resumable, body);
        // How much each network brings, where the second carries part of it (gaps row K4).
        let settings = view.read(cx).backend.settings();
        let body = body.children(networks_line(
            &look,
            &connections,
            &speeds,
            [
                settings.network_name(&settings.second_path),
                settings.network_name(&settings.third_path),
            ],
            row.state == State::Running && !joining,
        ));
        let body = body.when(!row.state.settled(), |d| {
            d.child(speed_line(&look, &self.line))
        });
        let body = self.limits(&look, &row, this_done, cx, body);
        let body = self.connections_or_swarm(&look, torrent, conn_rows, cx, body);
        let body = body.child(self.buttons(&look, &row, cx));

        let root = div()
            .id(("progress", id as usize))
            .role(Role::Dialog)
            .map(crate::window::mirror::text)
            .aria_label(title(&named))
            .track_focus(&self.focus)
            // Escape closes it, as its close button does; the list then
            // knows it was closed, and does not open it again by itself.
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, window, _| {
                if event.keystroke.key == "escape" {
                    window.remove_window();
                }
            }))
            // "When this download is done"'s choice (gaps row U36).
            .on_action(
                cx.listener(|this, pick: &crate::widgets::popup::Pick, _, cx| {
                    if pick.field == THIS_DONE {
                        this.this_done(pick.index, cx);
                    }
                }),
            )
            .size_full()
            .bg(theme.window);
        // A phone's: its own top bar, the file's name and Back, as every dialog's there.
        let root = native::phone_framed(root, named.clone(), theme)
            .child(crate::widgets::fit::fitted(body));
        crate::window::safe::kept_clear(root, window).into_any_element()
    }
}

/// The progress window's lines that open, and whether each is (gaps row E9).
#[derive(Default)]
struct Opened {
    limit: bool,
    done: bool,
}

/// Opens the progress window of `id`, `nth` of those open (each a step down
/// and across from the one before, as windows cascade).
pub fn open(
    view: WeakEntity<Downloads>,
    id: Id,
    nth: usize,
    main: gpui::AnyWindowHandle,
    cx: &mut App,
) -> Option<gpui::AnyWindowHandle> {
    let downloads = view.upgrade()?;
    let (row_limit, name) = downloads
        .read(cx)
        .rows
        .iter()
        .find(|r| r.id == id)
        .map(|r| (r.speed_limit, dm_shell::app::names::name_of(r)))
        .unwrap_or_default();
    let step = 28.0 * (nth % 8) as f32;
    // Over dm's own window, below its bars, each a step down and across.
    let main = main.update(cx, |_, window, _| window.bounds()).ok();
    let origin = main
        .map(|b| b.origin + point(px(24.0 + step), px(96.0 + step)))
        .unwrap_or_else(|| point(px(120.0 + step), px(120.0 + step)));
    let (width, height) = crate::widgets::native::WindowSize::Progress.here();
    let bounds = Bounds::new(origin, size(px(width), px(height)));
    let opened = crate::window::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(title(&name)),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            // Known by its name.
            Screens::named(window.window_handle(), "Download progress", cx);
            // The keyboard in the window as it opens, so Escape reaches it.
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            let own_limit = cx.new(|cx| {
                let mut s = InputState::new(window, cx);
                let shown = if row_limit > 0 { row_limit } else { 500 };
                s.set_value(shown.to_string(), window, cx);
                s
            });
            let progress = cx.new(|cx| {
                // Drawn again as the list changes.
                cx.observe(&downloads, |_, _, cx| cx.notify()).detach();
                // A limit typed while it is on is put in force.
                cx.subscribe_in(
                    &own_limit,
                    window,
                    |this: &mut Progress, _, event, _, cx| {
                        if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                            let on = this
                                .view
                                .upgrade()
                                .and_then(|v| {
                                    v.read(cx)
                                        .rows
                                        .iter()
                                        .find(|r| r.id == this.id)
                                        .map(|r| r.speed_limit > 0)
                                })
                                .unwrap_or(false);
                            if on {
                                this.limit(true, cx);
                            }
                        }
                    },
                )
                .detach();
                Progress {
                    view,
                    id,
                    rates: ConnRates::default(),
                    resumable_seen: None,
                    pieces: Vec::new(),
                    resumable: None,
                    inspected: None,
                    inspecting: false,
                    connections: false,
                    opened: Opened::default(),
                    line: Default::default(),
                    own_limit,
                    focus,
                    screen,
                }
            });
            cx.new(|cx| gpui_component::Root::new(progress, window, cx))
        },
    );
    match opened {
        Ok(handle) => Some(handle.into()),
        Err(e) => {
            dm_core::note!(error, "dm-gpui: a progress window could not be opened: {e}");
            None
        }
    }
}

/// The words a progress window is found by.
pub fn title(name: &str) -> SharedString {
    tf("Download progress — {}", &[&name]).into()
}
