// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! View › Statistics (gaps row W15): how many downloads finished and their bytes; by day over
//! the last 30 days, by site (the 10 largest) and by category, each as bars; and the average
//! and best speed since dm opened. Worked out as it opens from the list dm already keeps
//! (`dm_desktop_base::statistics`); nothing new is written.

use dm_desktop_base::format;
use dm_desktop_base::i18n::{t, tf};
use dm_desktop_base::statistics::{day_words, fraction, Share, Statistics};
use gpui::{
    div, prelude::*, size, Action, App, Bounds, Context, KeyDownEvent, Role, Window, WindowBounds,
    WindowOptions,
};

use crate::widgets::native;
use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// View › Statistics.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct ShowStatistics;

/// The labels of the figures' column.
const LABELS: [&str; 4] = [
    "Files finished",
    "Downloaded in all",
    "Average speed",
    "Best speed",
];

/// The Statistics window's view: the figures as they were when it opened.
pub struct StatisticsWindow {
    figures: Statistics,
    focus: gpui::FocusHandle,
    screen: Screen,
}

impl StatisticsWindow {
    /// A figure: its label in the labels' column, `words` beside it.
    fn figure(&self, label: &'static str, words: String) -> gpui::Div {
        let width = native::label_width(&LABELS.map(t));
        native::row(
            Some(t(label)),
            width,
            &self.screen,
            div().child(self.screen.label(words)),
        )
    }

    /// The last 30 days as bars, the oldest at the left, with the first and the last day
    /// under them and the busiest said.
    fn by_day(&self, theme: Theme) -> gpui::Div {
        let screen = &self.screen;
        let days = &self.figures.days;
        let top = self.figures.busiest_day();
        let busiest = days.iter().rev().find(|d| d.bytes == top && top > 0);
        div()
            .flex()
            .flex_col()
            .gap(px(space::XS))
            .child(
                div()
                    .h(px(72.0))
                    .flex()
                    .items_end()
                    .gap(px(space::XXS))
                    .p(px(space::XS))
                    .rounded(px(space::CORNER_S))
                    .bg(theme.stripe)
                    .children(days.iter().map(|day| {
                        div()
                            .flex_1()
                            .h(gpui::relative(fraction(day.bytes, top)))
                            .bg(theme.accent_text)
                    })),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(native::SMALL_TEXT))
                    .text_color(theme.muted)
                    .children(
                        [days.first(), days.last()]
                            .into_iter()
                            .flatten()
                            .map(|d| screen.label(day_words(d.date))),
                    ),
            )
            .when_some(busiest, |d, day| {
                d.child(native::note(
                    tf(
                        "Busiest day: {}, {}",
                        &[&day_words(day.date), &format::bytes(day.bytes)],
                    ),
                    theme.muted,
                    screen,
                ))
            })
    }

    /// Each share as a line: its name, a bar against the largest, its bytes.
    fn shares(&self, shares: &[Share], theme: Theme) -> gpui::Div {
        let screen = &self.screen;
        let top = shares.first().map_or(0, |s| s.bytes);
        div()
            .flex()
            .flex_col()
            .gap(px(space::XS))
            .children(shares.iter().map(|share| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::M))
                    .child(
                        div()
                            .w(px(150.0))
                            .flex_none()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(screen.label(share.name.clone())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(8.0))
                            .rounded_full()
                            .bg(theme.stripe)
                            .child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .bg(theme.accent_text)
                                    .w(gpui::relative(fraction(share.bytes, top))),
                            ),
                    )
                    .child(
                        div()
                            .w(px(80.0))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .text_color(theme.muted)
                            .child(screen.label(format::bytes(share.bytes))),
                    )
            }))
    }

    /// A section's heading.
    fn heading(&self, words: &'static str, theme: Theme) -> gpui::Div {
        native::heading(t(words), theme, &self.screen)
    }
}

impl Render for StatisticsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let f = &self.figures;
        let unmeasured = || t("not measured yet").to_string();
        let body = native::body(theme)
            .child(self.figure("Files finished", f.files.to_string()))
            .child(self.figure("Downloaded in all", format::bytes(f.bytes)))
            .when(f.files == 0, |d| {
                d.child(native::note(
                    t("Nothing has finished yet."),
                    theme.muted,
                    &screen,
                ))
            })
            .child(self.heading("By day, the last 30 days", theme))
            .child(self.by_day(theme))
            .child(self.heading("By site, the 10 largest", theme))
            .child(self.shares(&f.sites, theme))
            .child(self.heading("By category", theme))
            .child(self.shares(&f.categories, theme))
            .child(self.heading("Speed", theme))
            .child(self.figure(
                "Average speed",
                f.average_speed.map(format::rate).unwrap_or_else(unmeasured),
            ))
            .child(self.figure(
                "Best speed",
                f.best_speed.map(format::rate).unwrap_or_else(unmeasured),
            ))
            .child(native::note(
                t("Speeds are measured while dm is open, and forgotten when it closes."),
                theme.muted,
                &screen,
            ))
            .child(native::note(
                t("Worked out from the finished downloads in the list; nothing new is kept."),
                theme.muted,
                &screen,
            ))
            .child(native::buttons(
                Vec::new(),
                vec![
                    native::button("close", t("Close").into(), false, theme, &screen)
                        .on_click(|_, window, _| window.remove_window())
                        .into_any_element(),
                ],
            ));
        let root = div()
            .id("statistics")
            .role(Role::Dialog)
            .aria_label(t("Statistics"))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, window, _| {
                if matches!(event.keystroke.key.as_str(), "escape" | "enter") {
                    window.remove_window();
                }
            }))
            .size_full()
            .bg(theme.window);
        let root = native::phone_framed(root, t("Statistics"), theme).child(
            div()
                .id("statistics-body")
                .size_full()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(body),
        );
        crate::window::safe::kept_clear(root, window)
    }
}

/// Opens Statistics with `figures`, centred, at its size in the table.
fn open(figures: Statistics, cx: &mut App) -> Option<gpui::AnyWindowHandle> {
    let (width, height) = native::WindowSize::Statistics.here();
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    let opened = crate::window::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Statistics").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            Screens::named(window.window_handle(), "Statistics", cx);
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            let view = cx.new(|_| StatisticsWindow {
                figures,
                focus,
                screen,
            });
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        },
    );
    match opened {
        Ok(handle) => Some(handle.into()),
        Err(e) => {
            dm_core::note!(error, "dm-gpui: Statistics could not be opened: {e}");
            None
        }
    }
}

impl Downloads {
    /// View › Statistics: the figures of the list as it is now, and of the speeds seen since
    /// dm opened; brought forward where it is open.
    pub(crate) fn open_statistics(&mut self, cx: &mut Context<Self>) {
        let open_now = cx.windows();
        if let Some(shown) = self
            .windows
            .statistics_window
            .filter(|w| open_now.contains(w))
        {
            let _ = shown.update(cx, |_, window, _| window.activate_window());
            return;
        }
        let figures = Statistics::now(self.rows.iter().map(|r| &**r), &self.status.speeds);
        let me = cx.entity().downgrade();
        cx.defer(move |cx| {
            let handle = open(figures, cx);
            let _ = me.update(cx, |me, _| me.windows.statistics_window = handle);
        });
    }
}
