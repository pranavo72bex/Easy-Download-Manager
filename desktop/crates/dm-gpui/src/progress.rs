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

use crate::a11y::Told;
use crate::list::Downloads;
use crate::native;
use crate::screen::{Screen, Screens};
use crate::theme::Theme;
use crate::zoom::px;

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
    own_limit: Entity<InputState>,
    /// The window's keyboard: Escape closes it, as its close button does.
    focus: gpui::FocusHandle,
    screen: Screen,
}

/// A bar, as the system draws a progress indicator: `fraction` filled, and
/// its value told to a screen reader (gaps row U23).
fn bar(id: impl Into<gpui::ElementId>, fraction: f32, theme: Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .progress_bar(t("Progress"), f64::from(fraction))
        .h(px(6.0))
        .flex_1()
        .rounded_full()
        .bg(theme.stripe)
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(theme.accent)
                .w(gpui::relative(fraction.clamp(0.0, 1.0))),
        )
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
            if stop {
                view.backend.pause(id);
            } else {
                match dm_shell::app::rules::go_on(&row.state, row.paused_by_schedule) {
                    GoOn::Retry => view.backend.retry(id),
                    // Held by the schedule: asked whether to start it
                    // outside it.
                    GoOn::Held => view.ask_outside(vec![id], cx),
                    GoOn::Resume | GoOn::Settled | GoOn::NotStopped => view.backend.resume(id),
                }
            }
            view.read();
            cx.notify();
        });
    }

    /// Cancel download: asked first.
    fn cancel(&mut self, cx: &mut Context<Self>) {
        let id = self.id;
        let _ = self.view.update(cx, |view, cx| view.ask_cancel(id, cx));
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
            view.backend.set_own_limit(id, if on { typed } else { 0 });
            view.read();
            cx.notify();
        });
    }
}

impl Render for Progress {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
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
                // Its download taken out of the list (removed, here or by a script): the
                // window closes, as the egui window's did, rather than stay open and empty.
                // One the list has not read yet is still in the queue, and waits.
                if view.backend.row(self.id).is_none() {
                    window.remove_window();
                }
                return div().into_any_element();
            };
            let named = view
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
        crate::screen::set_title(
            window,
            &title_with_percent(
                tf("Download progress — {}", &[&named]),
                row.progress.downloaded,
                row.progress.total,
            ),
            cx,
        );
        let p = &row.progress;
        let running = row.state == State::Running;
        let width = native::label_width(&[
            t("Status"),
            t("File size"),
            t("Downloaded"),
            t("Can resume"),
        ]);
        let line = |label: &'static str, words: String| {
            native::row(
                Some(t(label)),
                width,
                &screen,
                div().child(screen.label(words)),
            )
        };
        let status = dm_desktop_base::rows::status_of(&row);
        let next_step = match &row.state {
            State::Failed(why) => format::next_step(why).map(t),
            _ => None,
        };
        let torrent = dm_core::is_torrent(&row.url);
        let fraction = p.fraction().unwrap_or(0.0) as f32;
        let pieces: Vec<(u64, u64)> = connections
            .iter()
            .map(|c| ((c.end - c.start).saturating_add(1), c.done))
            .collect();
        let multi = p.total.is_some() && connections.len() > 1;
        let own_on = row.speed_limit > 0;
        let settled = row.state.settled();
        let this_done = view
            .read(cx)
            .session
            .this_done
            .get(&id)
            .copied()
            .unwrap_or_default();

        // Connections, when open: each piece's bar and its own speed.
        let conn_rows = (self.connections && !torrent).then(|| {
            let held = format::pieces_shown_open(
                &pieces,
                &connections.iter().map(|c| c.started).collect::<Vec<_>>(),
                p.downloaded,
            );
            let finished = pieces
                .iter()
                .zip(&held)
                .filter(|((span, _), done)| **done >= *span)
                .count();
            let mut lines: Vec<gpui::AnyElement> = Vec::new();
            if connections.is_empty() {
                lines.push(
                    native::note(
                        if joining {
                            t("Every piece is in; putting them together.")
                        } else {
                            t("Nothing fetched yet.")
                        },
                        theme.muted,
                        &screen,
                    )
                    .into_any_element(),
                );
            } else {
                lines.push(
                    native::note(
                        dm_desktop_base::i18n::tn(
                            "{} of {} piece done||{} of {} pieces done",
                            connections.len() as u64,
                            &[&finished, &connections.len()],
                        ),
                        theme.muted,
                        &screen,
                    )
                    .into_any_element(),
                );
            }
            // Each path's share, where a second network is in use (Options › Advanced): the
            // pieces it took, by what they hold now (gaps row U38).
            if connections.iter().any(|c| c.second) {
                let came: u64 = held.iter().sum();
                let over_second: u64 = connections
                    .iter()
                    .zip(&held)
                    .filter(|(c, _)| c.second)
                    .map(|(_, &done)| done)
                    .sum();
                lines.push(
                    native::note(
                        dm_desktop_base::i18n::tf(
                            "{} of {} came over the second network",
                            &[&format::bytes(over_second), &format::bytes(came)],
                        ),
                        theme.muted,
                        &screen,
                    )
                    .into_any_element(),
                );
            }
            for (n, ((c, (span, _)), &done)) in
                connections.iter().zip(&pieces).zip(&held).enumerate()
            {
                let (words, fill) = piece_words(*span, done, c.started, running, p.total.is_some());
                let rate = speeds.get(n).copied().unwrap_or(0.0);
                lines.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_size(px(native::SMALL_TEXT))
                        .child(
                            div()
                                .w(px(22.0))
                                .text_color(theme.muted)
                                .child(screen.label(format!("{}", n + 1))),
                        )
                        .when_some(fill, |d, fill| {
                            d.child(
                                div()
                                    .w(px(160.0))
                                    .flex()
                                    .child(bar(("piece", n), fill, theme)),
                            )
                        })
                        .child(div().text_color(theme.muted).child(screen.label(words)))
                        .when(rate > 0.0 && done < *span && running, |d| {
                            d.child(
                                div()
                                    .text_color(theme.muted)
                                    .child(screen.label(format::rate(rate))),
                            )
                        })
                        .into_any_element(),
                );
            }
            lines
        });

        div()
            .id(("progress", id as usize))
            .role(Role::Dialog)
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
            .on_action(cx.listener(|this, pick: &crate::popup::Pick, _, cx| {
                if pick.field == THIS_DONE {
                    this.this_done(pick.index, cx);
                }
            }))
            .size_full()
            .bg(theme.window)
            .child(crate::fit::fitted(
                native::body(theme)
                    .child(
                        div()
                            .text_size(px(native::SMALL_TEXT))
                            .text_color(theme.muted)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(screen.label(format::shown_url(&row.url))),
                    )
                    .child(line("Status", status))
                    .when_some(next_step, |d, step| {
                        d.child(native::row(
                            None,
                            width,
                            &screen,
                            native::note(step, theme.muted, &screen),
                        ))
                    })
                    .child(line(
                        "File size",
                        p.total
                            .map(format::bytes)
                            .unwrap_or_else(|| t("unknown").into()),
                    ))
                    .child(line("Downloaded", format::bytes(p.downloaded)))
                    .child(line(
                        "Speed",
                        if running && !joining {
                            format::rate(p.speed)
                        } else {
                            String::new()
                        },
                    ))
                    .child(line(
                        "Time left",
                        if running {
                            p.eta.map(format::span).unwrap_or_default()
                        } else {
                            String::new()
                        },
                    ))
                    .when_some(can_resume_words(&row, resumable), |d, words| {
                        d.child(line("Can resume", words.to_string()))
                    })
                    // A piece for each connection where the pieces are known;
                    // a plain bar where they are not.
                    .child(if multi {
                        let total = p.total.unwrap_or(1).max(1) as f32;
                        div()
                            // One bar to a screen reader, at how far the whole file is.
                            .id("pieces")
                            .progress_bar(t("Progress"), f64::from(fraction))
                            .h(px(8.0))
                            .flex()
                            .gap(px(1.0))
                            .children(pieces.iter().map(|(span, done)| {
                                div()
                                    // As wide as its share of the file.
                                    .flex_grow(*span as f32 / total)
                                    .flex_basis(px(0.0))
                                    .h_full()
                                    .bg(theme.stripe)
                                    .child(div().h_full().bg(theme.accent).w(gpui::relative(
                                        (*done as f32 / (*span).max(1) as f32).clamp(0.0, 1.0),
                                    )))
                            }))
                            .into_any_element()
                    } else {
                        div()
                            .flex()
                            .child(bar("bar", fraction, theme))
                            .into_any_element()
                    })
                    // This download's own limit, beside the one for all.
                    .when(!settled, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(native::LABEL_GAP))
                                .child(
                                    native::checkbox(
                                        "own-limit",
                                        t("Limit this download to").into(),
                                        own_on,
                                        theme,
                                        &screen,
                                    )
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.limit(!own_on, cx)),
                                    ),
                                )
                                .child(div().w(px(90.0)).child(native::field(
                                    &self.own_limit,
                                    "Limit this download to",
                                    theme,
                                )))
                                .child(screen.label(t("KB/s"))),
                        )
                    })
                    // What dm does when this download is done (gaps row U36).
                    .when(!settled, |d| {
                        d.child(native::row(
                            Some(t("When this download is done")),
                            width,
                            &screen,
                            crate::popup::button(
                                THIS_DONE,
                                t(this_done.english()).into(),
                                this_done_choices()
                                    .iter()
                                    .map(|c| t(c.english()).into())
                                    .collect(),
                                this_done_choices().iter().position(|c| *c == this_done),
                                theme,
                                &screen,
                            ),
                        ))
                    })
                    // Shut until asked for; a torrent's swarm in its place.
                    .when(!torrent, |d| {
                        let open = self.connections;
                        d.child(
                            div()
                                .id("connections")
                                .role(Role::Button)
                                .aria_label(t("Connections"))
                                // Open or shut, as its arrow shows (gaps row U23).
                                .aria_expanded(open)
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .child(div().text_color(theme.muted).child(if open {
                                    "▾"
                                } else {
                                    "▸"
                                }))
                                .child(screen.label(t("Connections")))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.connections = !this.connections;
                                    cx.notify();
                                })),
                        )
                        .children(conn_rows.into_iter().flatten())
                    })
                    .when(torrent, |d| {
                        d.children(dm_desktop_base::rows::swarm_of(id).map(|swarm| {
                            div().flex().flex_col().children(
                                swarm_lines(&swarm)
                                    .into_iter()
                                    .map(|l| native::note(l, theme.muted, &screen)),
                            )
                        }))
                    })
                    .child(native::buttons(
                        Vec::new(),
                        [
                            matches!(row.state, State::Running | State::Queued).then(|| {
                                native::button(
                                    "stop",
                                    t("Stop download").into(),
                                    false,
                                    theme,
                                    &screen,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.go(true, cx)))
                                .into_any_element()
                            }),
                            matches!(row.state, State::Failed(_)).then(|| {
                                native::button("retry", t("Retry").into(), false, theme, &screen)
                                    .on_click(cx.listener(|this, _, _, cx| this.go(false, cx)))
                                    .into_any_element()
                            }),
                            (row.state == State::Paused).then(|| {
                                let word = if row.paused_by_schedule {
                                    "Start now"
                                } else {
                                    "Resume"
                                };
                                native::button("resume", t(word).into(), false, theme, &screen)
                                    .on_click(cx.listener(|this, _, _, cx| this.go(false, cx)))
                                    .into_any_element()
                            }),
                            (!settled).then(|| {
                                native::button(
                                    "cancel",
                                    t("Cancel download").into(),
                                    false,
                                    theme,
                                    &screen,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx)))
                                .into_any_element()
                            }),
                        ]
                        .into_iter()
                        .flatten()
                        .collect(),
                    )),
            ))
            .into_any_element()
    }
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
    let bounds = Bounds::new(origin, size(px(480.0), px(320.0)));
    let opened = crate::screen::open_window(
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
            // Known by its name, as the egui window's progress windows are.
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
            eprintln!("dm-gpui: a progress window could not be opened: {e}");
            None
        }
    }
}

/// The words a progress window is found by.
pub fn title(name: &str) -> SharedString {
    tf("Download progress — {}", &[&name]).into()
}
