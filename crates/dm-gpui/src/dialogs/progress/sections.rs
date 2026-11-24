// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The progress window's sections, top to bottom, each a piece of the
//! window that `render` puts together in this order.

use super::*;

/// What each section is drawn with: the look in force, the screen its words
/// are recorded on, and the labels' column.
pub(super) struct Look {
    theme: Theme,
    screen: Screen,
    width: f32,
}

impl Look {
    /// The sections' look, with `screen` to record their words on.
    pub(super) fn new(theme: Theme, screen: Screen) -> Self {
        let width = native::label_width(&[
            t("Status"),
            t("File size"),
            t("Downloaded"),
            t("Can resume"),
            t("Speed"),
            t("Time left"),
            t("When this download is done"),
        ]);
        Self {
            theme,
            screen,
            width,
        }
    }

    // A row of words: `label`'s in the labels' column, `words` beside them.
    fn line(&self, label: &'static str, words: String) -> gpui::Div {
        native::row(
            Some(t(label)),
            self.width,
            &self.screen,
            // Wrapped at the window's edge where a fact is longer than its line (gaps row E2).
            div().min_w_0().child(self.screen.label(words)),
        )
    }
}

/// Connections, when open: each piece's bar and its own speed.
pub(super) fn connection_lines(
    look: &Look,
    row: &dm_core::Entry,
    connections: &[dm_core::Connection],
    pieces: &[(u64, u64)],
    speeds: &[f64],
    joining: bool,
) -> Vec<gpui::AnyElement> {
    let (theme, screen) = (look.theme, &look.screen);
    let p = &row.progress;
    let running = row.state == State::Running;
    let held = format::pieces_shown_open(
        pieces,
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
                screen,
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
                screen,
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
                screen,
            )
            .into_any_element(),
        );
    }
    for (n, ((c, (span, _)), &done)) in connections.iter().zip(pieces).zip(&held).enumerate() {
        let (words, fill) = piece_words(*span, done, c.started, running, p.total.is_some());
        let rate = speeds.get(n).copied().unwrap_or(0.0);
        lines.push(
            div()
                .flex()
                .items_center()
                .gap(px(space::M))
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
}

/// The address, the status (and under a failure its next step), the sizes, the speed,
/// the time left, and whether it can resume.
pub(super) fn facts(
    look: &Look,
    row: &dm_core::Entry,
    joining: bool,
    resumable: Option<bool>,
    body: gpui::Div,
) -> gpui::Div {
    let (theme, screen) = (look.theme, &look.screen);
    let p = &row.progress;
    let running = row.state == State::Running;
    let status = dm_desktop_base::rows::status_of(row);
    let next_step = match &row.state {
        State::Failed(why) => format::next_step(why).map(t),
        _ => None,
    };
    body.child(
        div()
            .text_size(px(native::SMALL_TEXT))
            .text_color(theme.muted)
            .whitespace_nowrap()
            .overflow_hidden()
            .text_ellipsis()
            .child(screen.label(format::shown_url(&row.url))),
    )
    .child(look.line("Status", status))
    .when_some(next_step, |d, step| {
        d.child(native::row(
            None,
            look.width,
            screen,
            native::note(step, theme.muted, screen),
        ))
    })
    .child(
        look.line(
            "File size",
            p.total
                .map(format::bytes)
                .unwrap_or_else(|| t("unknown").into()),
        ),
    )
    .child(look.line("Downloaded", format::bytes(p.downloaded)))
    .child(look.line(
        "Speed",
        if running && !joining {
            format::rate(p.speed)
        } else {
            String::new()
        },
    ))
    .child(look.line(
        "Time left",
        if running {
            p.eta.map(format::span).unwrap_or_default()
        } else {
            String::new()
        },
    ))
    .when_some(can_resume_words(row, resumable), |d, words| {
        d.child(look.line("Can resume", words.to_string()))
    })
}

/// How many bars the speed line is drawn as: one for each five seconds of its five minutes.
const LINE_BARS: usize = 60;

/// The speed over the last five minutes (gaps row W15): a bar for each five seconds, as tall
/// as it is against the fastest, the latest at the right.
pub(super) fn speed_line(look: &Look, line: &dm_desktop_base::statistics::SpeedLine) -> gpui::Div {
    let theme = look.theme;
    div()
        .flex()
        .flex_col()
        .gap(px(space::XS))
        .child(native::note(
            t("Speed over the last 5 minutes"),
            theme.muted,
            &look.screen,
        ))
        .child(
            div()
                .h(px(36.0))
                .flex()
                .items_end()
                .gap(px(space::LINE))
                .p(px(space::XXS))
                .rounded(px(space::CORNER_S))
                .bg(theme.stripe)
                .children(
                    line.bars(LINE_BARS)
                        .into_iter()
                        .map(|tall| div().flex_1().h(gpui::relative(tall)).bg(theme.accent)),
                ),
        )
}

/// A piece for each connection where the pieces are known;
/// a plain bar where they are not.
pub(super) fn whole_bar(
    look: &Look,
    row: &dm_core::Entry,
    pieces: &[(u64, u64)],
    multi: bool,
) -> gpui::AnyElement {
    let theme = look.theme;
    let p = &row.progress;
    let fraction = p.fraction().unwrap_or(0.0) as f32;
    if multi {
        let total = p.total.unwrap_or(1).max(1) as f32;
        div()
            // One bar to a screen reader, at how far the whole file is.
            .id("pieces")
            .progress_bar(t("Progress"), f64::from(fraction))
            .h(px(8.0))
            .flex()
            .gap(px(space::LINE))
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
    }
}

impl Progress {
    /// This download's own limit, and what dm does when it is done.
    pub(super) fn limits(
        &self,
        look: &Look,
        row: &dm_core::Entry,
        this_done: ThisDone,
        cx: &mut Context<Self>,
        body: gpui::Div,
    ) -> gpui::Div {
        let (theme, screen) = (look.theme, &look.screen);
        let own_on = row.speed_limit > 0;
        let settled = row.state.settled();
        body.when(!settled, |d| {
            d.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(native::LABEL_GAP))
                    .child(
                        native::switch_inline(
                            "own-limit",
                            t("Limit this download to").into(),
                            own_on,
                            theme,
                            screen,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.limit(!own_on, cx))),
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
                look.width,
                screen,
                crate::widgets::popup::button(
                    THIS_DONE,
                    t(this_done.english()).into(),
                    this_done_choices()
                        .iter()
                        .map(|c| t(c.english()).into())
                        .collect(),
                    this_done_choices().iter().position(|c| *c == this_done),
                    theme,
                    screen,
                ),
            ))
        })
    }

    /// Connections, shut until asked for; a torrent's swarm in its place.
    pub(super) fn connections_or_swarm(
        &self,
        look: &Look,
        torrent: bool,
        conn_rows: Option<Vec<gpui::AnyElement>>,
        cx: &mut Context<Self>,
        body: gpui::Div,
    ) -> gpui::Div {
        let (theme, screen) = (look.theme, &look.screen);
        let id = self.id;
        body.when(!torrent, |d| {
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
                    .gap(px(space::XS))
                    .child(
                        div()
                            .text_color(theme.muted)
                            .child(if open { "▾" } else { "▸" }),
                    )
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
                        .map(|l| native::note(l, theme.muted, screen)),
                )
            }))
        })
    }

    /// Stop, the failure's next step, Open folder and Open, Retry, Resume, and Cancel.
    pub(super) fn buttons(
        &self,
        look: &Look,
        row: &dm_core::Entry,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let (theme, screen) = (look.theme, &look.screen);
        let step = match &row.state {
            State::Failed(why) => format::step_button(why),
            _ => None,
        };
        let settled = row.state.settled();
        native::buttons(
            Vec::new(),
            [
                matches!(row.state, State::Running | State::Queued).then(|| {
                    native::button("stop", t("Stop download").into(), false, theme, screen)
                        .on_click(cx.listener(|this, _, _, cx| this.go(true, cx)))
                        .into_any_element()
                }),
                // The next step the failure names, as a button beside Retry where
                // dm can take it (gaps row U10).
                step.map(|step| {
                    native::button("next-step", t(step.words()).into(), false, theme, screen)
                        .on_click(cx.listener(move |this, _, _, cx| this.take_step(step, cx)))
                        .into_any_element()
                }),
                // Finished with this window open: it is Download complete itself
                // (gaps row U11, B63).
                (row.state == State::Completed).then(|| {
                    native::button("open-folder", t("Open folder").into(), false, theme, screen)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.finished_with(
                                dm_shell::app::commands::Command::OpenFolder,
                                window,
                                cx,
                            )
                        }))
                        .into_any_element()
                }),
                (row.state == State::Completed).then(|| {
                    native::button("open", t("Open").into(), true, theme, screen)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.finished_with(
                                dm_shell::app::commands::Command::OpenFile,
                                window,
                                cx,
                            )
                        }))
                        .into_any_element()
                }),
                matches!(row.state, State::Failed(_)).then(|| {
                    native::button("retry", t("Retry").into(), false, theme, screen)
                        .on_click(cx.listener(|this, _, _, cx| this.go(false, cx)))
                        .into_any_element()
                }),
                (row.state == State::Paused).then(|| {
                    let word = if row.paused_by_schedule {
                        "Start now"
                    } else {
                        "Resume"
                    };
                    native::button("resume", t(word).into(), false, theme, screen)
                        .on_click(cx.listener(|this, _, _, cx| this.go(false, cx)))
                        .into_any_element()
                }),
                (!settled).then(|| {
                    native::button("cancel", t("Cancel download").into(), false, theme, screen)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel(cx)))
                        .into_any_element()
                }),
            ]
            .into_iter()
            .flatten()
            .collect(),
        )
    }
}
