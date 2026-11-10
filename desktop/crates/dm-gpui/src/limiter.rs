// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Speed limiter (B20, B52), a window of its own: the speed limit, a
//! download limit for a link with a fair-use allowance (at most so many MB
//! in each period of so many hours), and a lower limit for set hours of
//! every day. OK puts them in force at once.

use dm_desktop_base::i18n::t;
use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Entity, FocusHandle, KeyDownEvent, Role,
    SharedString, WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::list::Downloads;
use crate::native;
use crate::screen::{Screen, Screens};
use crate::theme::Theme;

/// The Speed limiter window's view.
pub struct Limiter {
    view: WeakEntity<Downloads>,
    limit: Entity<InputState>,
    mb: Entity<InputState>,
    hours: Entity<InputState>,
    slow: dm_shell::settings::SlowHours,
    slow_from: Entity<InputState>,
    slow_to: Entity<InputState>,
    slow_limit: Entity<InputState>,
    says: Option<SharedString>,
    done: bool,
    focus: FocusHandle,
    screen: Screen,
}

/// A whole number as typed, within `range`; what is not one is `fallback`.
fn number(
    input: &Entity<InputState>,
    range: std::ops::RangeInclusive<u64>,
    fallback: u64,
    cx: &App,
) -> u64 {
    input
        .read(cx)
        .value()
        .trim()
        .parse::<u64>()
        .map(|n| n.clamp(*range.start(), *range.end()))
        .unwrap_or(fallback)
}

impl Limiter {
    /// OK: the limits in force at once, or the slower hours' times said.
    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let backend = view.read(cx).backend.clone();
        let now = backend.settings();
        self.slow.from = self.slow_from.read(cx).value().trim().to_string();
        self.slow.to = self.slow_to.read(cx).value().trim().to_string();
        self.slow.limit = number(&self.slow_limit, 1..=10_000_000, now.slow_hours.limit, cx);
        if let Some(why) = dm_desktop_base::scheduler::slow_hours_problem(&self.slow) {
            self.says = Some(why.into());
            cx.notify();
            return;
        }
        let mut wanted = now.clone();
        wanted.speed_limit = number(&self.limit, 0..=1024 * 1024, now.speed_limit, cx);
        wanted.limit_mb = number(&self.mb, 0..=1024 * 1024 * 1024, now.limit_mb, cx);
        wanted.limit_hours = number(&self.hours, 1..=24 * 31, now.limit_hours, cx);
        wanted.slow_hours = self.slow.clone();
        match backend.set_settings(wanted) {
            Ok(_) => {
                self.done = true;
                // Put in force at once, by the hours as they are now.
                let _ = self.view.update(cx, |view, cx| {
                    view.session.forget_limit();
                    // Limit speed's tick in the menu bar.
                    view.refresh_menus(cx);
                    cx.notify();
                });
                window.remove_window();
            }
            Err(e) => {
                self.says = Some(e.into());
                cx.notify();
            }
        }
    }

    /// Cancel, Escape or the close button.
    fn cancel(&mut self, window: &mut Window) {
        self.done = true;
        window.remove_window();
    }
}

impl Render for Limiter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let slow_on = self.slow.enabled;
        let labels = [
            t("Speed limit (KB/s, 0 for none)"),
            t("Download at most this many MB (0 for no limit)"),
            t("in each period of this many hours"),
        ];
        let width = native::label_width(&labels);
        let number_field = |input: &Entity<InputState>, name: &'static str| {
            div().w(px(110.0)).child(native::field(input, name, theme))
        };
        div()
            .id("limiter")
            .role(Role::Dialog)
            .aria_label(t("Speed limiter"))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => this.cancel(window),
                    "enter" => this.ok(window, cx),
                    _ => {}
                }
            }))
            .size_full()
            .bg(theme.window)
            .child(crate::fit::fitted(
                native::body(theme)
                    .child(native::row(
                        Some(labels[0]),
                        width,
                        &screen,
                        number_field(&self.limit, "Speed limit (KB/s, 0 for none)"),
                    ))
                    // Download limits, for a link with a fair-use allowance:
                    // downloads are held once it is used and go on after.
                    .child(native::row(
                        Some(labels[1]),
                        width,
                        &screen,
                        number_field(&self.mb, "Download at most this many MB (0 for no limit)"),
                    ))
                    .child(native::row(
                        Some(labels[2]),
                        width,
                        &screen,
                        number_field(&self.hours, "in each period of this many hours"),
                    ))
                    // A lower limit for set hours of every day.
                    .child(native::row(
                        None,
                        width,
                        &screen,
                        native::checkbox(
                            "slow",
                            t("Slower between set hours").into(),
                            slow_on,
                            theme,
                            &screen,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.slow.enabled = !this.slow.enabled;
                            cx.notify();
                        })),
                    ))
                    .child(native::row(
                        None,
                        width,
                        &screen,
                        div()
                            .when(!slow_on, |d| d.opacity(0.45).occlude())
                            .flex()
                            .flex_col()
                            .gap(px(native::ROW_GAP))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(native::LABEL_GAP))
                                    .child(screen.label(t("Start at")))
                                    .child(div().w(px(70.0)).child(native::field(
                                        &self.slow_from,
                                        "Start at",
                                        theme,
                                    )))
                                    .child(div().w(px(8.0)))
                                    .child(screen.label(t("Stop at")))
                                    .child(div().w(px(70.0)).child(native::field(
                                        &self.slow_to,
                                        "Stop at",
                                        theme,
                                    ))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(native::LABEL_GAP))
                                    .child(screen.label(t("Speed limit in those hours (KB/s)")))
                                    .child(number_field(
                                        &self.slow_limit,
                                        "Speed limit in those hours (KB/s)",
                                    )),
                            ),
                    ))
                    .when_some(self.says.clone(), |d, says| {
                        d.child(native::note(says, theme.warning, &screen))
                    })
                    .child(native::buttons(
                        Vec::new(),
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            native::button("ok", t("OK").into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx)))
                                .into_any_element(),
                        ],
                    )),
            ))
    }
}

/// Opens the Speed limiter.
pub fn open(view: WeakEntity<Downloads>, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, cx));
}

fn open_now(view: WeakEntity<Downloads>, cx: &mut App) {
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let settings = downloads.read(cx).backend.settings();
    let bounds = Bounds::centered(None, size(px(560.0), px(300.0)), cx);
    let opened = crate::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Speed limiter").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let field = |value: String, window: &mut Window, cx: &mut App| {
                cx.new(|cx| {
                    let mut s = InputState::new(window, cx);
                    s.set_value(value, window, cx);
                    s
                })
            };
            let limit = field(settings.speed_limit.to_string(), window, cx);
            let mb = field(settings.limit_mb.to_string(), window, cx);
            let hours = field(settings.limit_hours.to_string(), window, cx);
            let slow_from = field(settings.slow_hours.from.clone(), window, cx);
            let slow_to = field(settings.slow_hours.to.clone(), window, cx);
            let slow_limit = field(settings.slow_hours.limit.to_string(), window, cx);
            // The limit takes the keyboard as the dialog opens, selected.
            limit.update(cx, |l, cx| {
                l.focus(window, cx);
                l.select_all(window, cx);
            });
            let limiter = cx.new(|cx| {
                // Return in a field is OK.
                for input in [&limit, &mb, &hours, &slow_from, &slow_to, &slow_limit] {
                    cx.subscribe_in(input, window, |this: &mut Limiter, _, event, window, cx| {
                        if let InputEvent::PressEnter { .. } = event {
                            this.ok(window, cx);
                        }
                    })
                    .detach();
                }
                Limiter {
                    view,
                    limit,
                    mb,
                    hours,
                    slow: settings.slow_hours.clone(),
                    slow_from,
                    slow_to,
                    slow_limit,
                    says: None,
                    done: false,
                    focus: cx.focus_handle(),
                    screen,
                }
            });
            crate::native::close_is_cancel(&limiter, window, cx);
            cx.new(|cx| gpui_component::Root::new(limiter, window, cx))
        },
    );
    if let Err(e) = opened {
        eprintln!("dm-gpui: the Speed limiter could not be opened: {e}");
    }
}

impl crate::native::CloseButton for Limiter {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
