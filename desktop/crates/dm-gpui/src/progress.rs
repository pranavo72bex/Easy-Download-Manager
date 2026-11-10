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
use gpui::{
    div, point, prelude::*, px, size, App, Bounds, Context, Entity, Role, SharedString, WeakEntity,
    Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::list::Downloads;
use crate::native;
use crate::screen::{Screen, Screens};
use crate::theme::Theme;

/// The progress window's view.
pub struct Progress {
    view: WeakEntity<Downloads>,
    id: Id,
    rates: ConnRates,
    /// What the server said of resuming, kept while the parts are joined.
    resumable_seen: Option<bool>,
    /// Connections is open.
    connections: bool,
    own_limit: Entity<InputState>,
    /// The window's keyboard: Escape closes it, as its close button does.
    focus: gpui::FocusHandle,
    screen: Screen,
}

/// A bar, as the system draws a progress indicator: `fraction` filled.
fn bar(fraction: f32, theme: Theme) -> gpui::Div {
    div()
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

impl Progress {
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
        let (row, backend, named) = {
            let view = view.read(cx);
            let Some(row) = view.rows.iter().find(|r| r.id == self.id).cloned() else {
                return div().into_any_element();
            };
            let settings = view.backend.settings();
            let mut categories = settings.categories();
            let named = dm_shell::app::names::name_it_will_get_after(
                &view.rows,
                &row,
                &settings.download_dir,
                &mut categories,
                None,
            );
            (row, view.backend.clone(), named)
        };
        let id = self.id;
        let connections = backend.connections(id);
        self.rates.sample(
            std::time::Instant::now(),
            &connections
                .iter()
                .map(|c| (c.start, c.done))
                .collect::<Vec<_>>(),
        );
        let speeds: Vec<f64> = connections
            .iter()
            .map(|c| self.rates.rate(c.start))
            .collect();
        // While its parts are joined the plan they are read from is gone:
        // the last answer stands.
        let joining = row.state == State::Running && dm_shell::app::rules::fetched_all(&row);
        let resumable = match backend.resumable(id) {
            Some(answer) => {
                self.resumable_seen = Some(answer);
                Some(answer)
            }
            None if joining => self.resumable_seen,
            None => None,
        };
        window.set_window_title(&title_with_percent(
            tf("Download progress — {}", &[&named]),
            row.progress.downloaded,
            row.progress.total,
        ));
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
                            d.child(div().w(px(160.0)).flex().child(bar(fill, theme)))
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
                        div().flex().child(bar(fraction, theme)).into_any_element()
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
                    // Shut until asked for; a torrent's swarm in its place.
                    .when(!torrent, |d| {
                        let open = self.connections;
                        d.child(
                            div()
                                .id("connections")
                                .role(Role::Button)
                                .aria_label(t("Connections"))
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
