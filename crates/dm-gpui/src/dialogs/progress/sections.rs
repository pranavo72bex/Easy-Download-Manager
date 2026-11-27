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
    /// The facts' names' column: as wide as the longest of them alone.
    facts: f32,
}

impl Look {
    /// The sections' look, with `screen` to record their words on.
    pub(super) fn new(theme: Theme, screen: Screen) -> Self {
        let width = native::label_width(&[
            t("File size"),
            t("Downloaded"),
            t("Can resume"),
            t("Networks"),
            t("When this download is done"),
        ]);
        let facts = native::label_width(&[
            t("File size"),
            t("Downloaded"),
            t("Can resume"),
            t("Networks"),
        ]);
        Self {
            theme,
            screen,
            width,
            facts,
        }
    }

    /// A fact, as every window reports one (`native::fact`).
    fn line(&self, label: &'static str, words: String) -> gpui::Div {
        // Wrapped at the window's edge where a fact is longer than its line (gaps row E2).
        native::fact(
            t(label),
            self.facts,
            self.theme,
            &self.screen,
            self.screen.label(words),
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
                    d.child(div().w(px(160.0)).flex().child(native::bar(
                        ("piece", n),
                        fill,
                        6.0,
                        theme,
                    )))
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

/// The address (and under a failure its next step), the sizes, and whether it can resume. Not
/// the status, the speed nor the time left: the figure line above says them (gaps row E9).
pub(super) fn facts(
    look: &Look,
    row: &dm_core::Entry,
    resumable: Option<bool>,
    body: gpui::Div,
) -> gpui::Div {
    let (theme, screen) = (look.theme, &look.screen);
    let p = &row.progress;
    let next_step = match &row.state {
        State::Failed(why) => format::next_step(why)
            .map(crate::platform::said_step)
            .map(t),
        _ => None,
    };
    body.child(
        native::cut(
            "progress-address",
            format::shown_url(&row.url),
            true,
            screen,
        )
        .text_size(px(native::SMALL_TEXT))
        .text_color(theme.muted),
    )
    // Under the address it is about, from the same edge.
    .when_some(next_step, |d, step| {
        d.child(native::note(step, theme.muted, screen))
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
    .when_some(can_resume_words(row, resumable), |d, words| {
        d.child(look.line("Can resume", words.to_string()))
    })
}

/// The figure the window is for, read first (gaps row E9, `docs/ui-design.md` rule 4): how far
/// the download is, its speed and the time left, in one line of larger type ("7% · 380 KB/s ·
/// 2 min left"); the status where it is not running.
pub(super) fn figure_line(look: &Look, row: &dm_core::Entry, joining: bool) -> gpui::Div {
    div()
        .text_size(px(native::FIGURE_TEXT))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .font_features(native::figures())
        .child(look.screen.label(figure_words(row, joining)))
}

/// The figure's words: how far, how fast, how long ("7% · 380 KB/s · 2 min left"); the
/// status where it is not running or its parts are being joined. The one wording the progress
/// window, View › Details and a phone's detail all lead with (gaps row E8).
pub(crate) fn figure_words(row: &dm_core::Entry, joining: bool) -> String {
    let p = &row.progress;
    let mut parts: Vec<String> = Vec::new();
    if let Some(f) = p.fraction() {
        parts.push(format!("{}%", (f * 100.0).floor() as u32));
    }
    if row.state == State::Running && !joining {
        if p.speed > 1.0 {
            parts.push(format::rate(p.speed));
        }
        if let Some(eta) = p.eta {
            parts.push(dm_desktop_base::i18n::tf("{} left", &[&format::span(eta)]));
        }
    } else {
        parts.push(dm_desktop_base::rows::status_of(row));
    }
    parts.join(" · ")
}

/// How much each network brings (gaps row K4): the computer's own connection and the second
/// network's, each its speed and its share of what is done, by the connections that went over
/// it; none where the second network carries none of it. A second network that brings nothing
/// while the download runs is said so: it shares one line with the first, or is not connected.
pub(super) fn networks_line(
    look: &Look,
    connections: &[dm_core::Connection],
    speeds: &[f64],
    named: [String; 2],
    running: bool,
) -> Option<gpui::Div> {
    let answers = [0, 1, 2].map(dm_core::answer_time);
    networks_words(
        connections,
        speeds,
        [&named[0], &named[1]],
        answers,
        running,
    )
    .map(|said| look.line("Networks", said))
}

/// [`networks_line`]'s words: each network's speed and share, the second and third by their
/// names `named`; none where no connection went over another network than the system's route
/// (gaps rows K3, K4).
pub(super) fn networks_words(
    connections: &[dm_core::Connection],
    speeds: &[f64],
    named: [&str; 2],
    answers: [Option<std::time::Duration>; 3],
    running: bool,
) -> Option<String> {
    if !connections.iter().any(|c| c.network > 0) {
        return None;
    }
    let (mut speed, mut done) = ([0.0f64; 3], [0u64; 3]);
    for (c, rate) in connections.iter().zip(speeds) {
        let path = usize::from(c.network.min(2));
        speed[path] += rate;
        done[path] += c.done;
    }
    let total = done.iter().sum::<u64>().max(1) as f64;
    let part = |path: usize, name: &str| {
        let mut said = format!(
            "{name}: {} · {}%",
            format::rate(speed[path]),
            (done[path] as f64 * 100.0 / total).round() as u32
        );
        // How long it takes to answer, where a request has gone over it (gaps row K4).
        if let Some(took) = answers[path] {
            let ms = took.as_millis().max(1).to_string();
            said.push_str(&format!(
                " · {}",
                dm_desktop_base::i18n::tf("answers in {} ms", &[&ms])
            ));
        }
        said
    };
    let mut parts = vec![part(0, t("This computer’s connection"))];
    for (path, name) in [(1, named[0]), (2, named[1])] {
        if connections.iter().any(|c| usize::from(c.network) == path) {
            parts.push(part(path, name));
        }
    }
    let mut said = parts.join("   ");
    if running
        && speed[0] > 1.0
        && connections.iter().any(|c| c.network > 0)
        && speed[1] + speed[2] < 1.0
    {
        said.push_str(&format!(
            "   ({})",
            t("the second network brings nothing now")
        ));
    }
    Some(said)
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
                        .map(|tall| div().flex_1().h(gpui::relative(tall)).bg(theme.accent_text)),
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
                    .child(div().h_full().bg(theme.accent_text).w(gpui::relative(
                        (*done as f32 / (*span).max(1) as f32).clamp(0.0, 1.0),
                    )))
            }))
            .into_any_element()
    } else {
        div()
            .flex()
            .child(native::bar("bar", fraction, 6.0, theme))
            .into_any_element()
    }
}

/// A line that opens (gaps row E9): its arrow, its `label`, and how it is set (`said`) in the
/// muted colour; told to a screen reader as a button, open or shut as its arrow shows.
fn opener(
    id: &'static str,
    label: &'static str,
    said: Option<String>,
    open: bool,
    look: &Look,
) -> gpui::Stateful<gpui::Div> {
    let (theme, screen) = (look.theme, &look.screen);
    div()
        .id(id)
        .map(|c| crate::widgets::native::ringed(c, theme))
        .role(Role::Button)
        .aria_label(t(label))
        // Open or shut, as its arrow shows (gaps row U23).
        .aria_expanded(open)
        .flex()
        // From the right where the language reads right to left: its arrow first there too.
        .map(crate::window::mirror::row)
        .items_center()
        .gap(px(space::XS))
        .child(crate::widgets::native::icon(
            if open {
                "icons/chevron-down.svg"
            } else {
                crate::window::mirror::forward()
            },
            native::ICON_SMALL,
            theme.muted,
        ))
        .child(screen.label(t(label)))
        .children(said.map(|said| {
            div()
                .map(|d| {
                    if crate::window::mirror::rtl() {
                        d.mr(px(space::S))
                    } else {
                        d.ml(px(space::S))
                    }
                })
                .text_color(theme.muted)
                .child(screen.label(said))
        }))
}

impl Progress {
    /// This download's own limit, and what dm does when it is done: each a line that says how
    /// it is set and opens to its control (gaps row E9).
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
        if row.state.settled() {
            return body;
        }
        let limit_said = if own_on {
            format!("{} {}", row.speed_limit, t("KB/s"))
        } else {
            t("No limit").to_string()
        };
        let limit_open = self.opened.limit;
        let done_open = self.opened.done;
        body.child(
            opener(
                "own-limit-line",
                "Speed limit",
                Some(limit_said),
                limit_open,
                look,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.opened.limit = !this.opened.limit;
                cx.notify();
            })),
        )
        .when(limit_open, |d| {
            d.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(native::LABEL_GAP))
                    .pl(px(space::L))
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
                    .child(
                        div()
                            .w(px(crate::widgets::native::NUMBER_FIELD))
                            .child(native::field(
                                &self.own_limit,
                                "Limit this download to",
                                theme,
                            )),
                    )
                    .child(screen.label(t("KB/s"))),
            )
        })
        // What dm does when this download is done (gaps row U36).
        .child(
            opener(
                "this-done-line",
                "When done",
                Some(t(this_done.english()).to_string()),
                done_open,
                look,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.opened.done = !this.opened.done;
                cx.notify();
            })),
        )
        .when(done_open, |d| {
            d.child(
                div().pl(px(space::L)).child(native::row(
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
                )),
            )
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
                opener("connections", "Connections", None, open, look).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.connections = !this.connections;
                        cx.notify();
                    },
                )),
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
            // Its page of the guide, at the leading end as each dialog's (gaps row E11).
            vec![native::help_button("download-window", theme, screen).into_any_element()],
            [
                matches!(row.state, State::Running | State::Queued).then(|| {
                    // The window's main action while it runs, filled (gaps row E9).
                    native::button("stop", t("Stop download").into(), true, theme, screen)
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
                    native::button("retry", t("Retry").into(), true, theme, screen)
                        .on_click(cx.listener(|this, _, _, cx| this.go(false, cx)))
                        .into_any_element()
                }),
                (row.state == State::Paused).then(|| {
                    let word = if row.paused_by_schedule {
                        "Start now"
                    } else {
                        "Resume"
                    };
                    native::button("resume", t(word).into(), true, theme, screen)
                        .on_click(cx.listener(|this, _, _, cx| this.go(false, cx)))
                        .into_any_element()
                }),
                (!settled).then(|| {
                    // It destroys what was fetched: plain, its words in the error colour
                    // (gaps row E9, `docs/ui-design.md` rule 3).
                    native::button("cancel", t("Cancel download").into(), false, theme, screen)
                        .text_color(theme.warning)
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

#[cfg(test)]
mod network_tests {
    use super::*;

    fn connection(done: u64, network: u8) -> dm_core::Connection {
        dm_core::Connection {
            start: 0,
            end: 0,
            done,
            started: true,
            second: network > 0,
            network,
        }
    }

    /// Each network's speed and share of what is done; said where the second brings nothing
    /// while it runs; nothing said where it carries none of the download (gaps row K4).
    #[test]
    fn each_networks_speed_and_share_are_said() {
        let both = [connection(600, 0), connection(400, 1)];
        let said = networks_words(
            &both,
            &[2_000_000.0, 1_000_000.0],
            ["iPhone USB", "Ethernet"],
            [Some(std::time::Duration::from_millis(40)), None, None],
            true,
        )
        .unwrap();
        // How long the first takes to answer, where it is known (gaps row K4).
        assert!(said.contains("answers in 40 ms"), "{said}");
        assert_eq!(said.matches("answers in").count(), 1, "{said}");
        assert!(
            said.contains("This computer’s connection") && said.contains("60%"),
            "{said}"
        );
        assert!(
            said.contains("iPhone USB") && said.contains("40%"),
            "{said}"
        );
        assert!(!said.contains("brings nothing"), "{said}");
        let idle = networks_words(
            &both,
            &[2_000_000.0, 0.0],
            ["iPhone USB", "Ethernet"],
            [None; 3],
            true,
        )
        .unwrap();
        assert!(idle.contains("brings nothing"), "{idle}");
        let first_only = [connection(600, 0)];
        assert_eq!(
            networks_words(
                &first_only,
                &[1.0],
                ["iPhone USB", "Ethernet"],
                [None; 3],
                true
            ),
            None
        );
        // A third network beside the second, each by its own name and share (gaps row K3).
        let three = [connection(500, 0), connection(300, 1), connection(200, 2)];
        let said = networks_words(
            &three,
            &[1.0e6, 1.0e6, 1.0e6],
            ["iPhone USB", "Ethernet"],
            [None; 3],
            true,
        )
        .unwrap();
        assert!(
            said.contains("iPhone USB") && said.contains("30%"),
            "{said}"
        );
        assert!(said.contains("Ethernet") && said.contains("20%"), "{said}");
    }
}

#[cfg(test)]
mod figure_tests {
    use super::*;

    fn row(state: State, speed: f64, eta: Option<u64>) -> dm_core::Entry {
        serde_json::from_value(serde_json::json!({
            "id": 1,
            "url": "http://e.com/f",
            "state": serde_json::to_value(state).unwrap(),
            "progress": {
                "downloaded": 70, "total": 1000, "speed": speed,
                "eta": eta.map(|s| serde_json::json!({"secs": s, "nanos": 0})),
            },
            "filename": "f",
        }))
        .unwrap()
    }

    /// The one figure the progress window and View › Details lead with: how far, how fast and
    /// how long while it runs; its status while its parts are joined or it is not running
    /// (gaps rows E8, E9).
    #[test]
    fn the_figure_says_how_far_how_fast_and_how_long() {
        let running = row(State::Running, 389_120.0, Some(120));
        let said = figure_words(&running, false);
        // KB is this computer's: 1024 bytes on Windows, 1000 elsewhere (gaps row U34).
        let speed = format::rate(389_120.0);
        assert_eq!(said, format!("7% · {speed} · 2m 0s left"));
        assert!(said.ends_with("left"), "{said}");
        let joining = figure_words(&running, true);
        assert!(!joining.contains("left"), "{joining}");
        let paused = figure_words(&row(State::Paused, 0.0, None), false);
        assert!(paused.starts_with("7% · "), "{paused}");
        assert!(!paused.contains("left"), "{paused}");
    }
}
