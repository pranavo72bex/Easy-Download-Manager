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
    div, prelude::*, size, App, Bounds, Context, Entity, FocusHandle, KeyDownEvent, SharedString,
    WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::widgets::native;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// The Speed limiter window's view.
pub struct Limiter {
    view: WeakEntity<Downloads>,
    limit: Entity<InputState>,
    mb: Entity<InputState>,
    hours: Entity<InputState>,
    slow: dm_shell::settings::SlowHours,
    slow_limit: Entity<InputState>,
    says: Option<SharedString>,
    done: bool,
    focus: FocusHandle,
    screen: Screen,
}

impl Limiter {
    /// A slow hour's time chosen from its pop-ups (gaps row U32).
    pub(crate) fn pick(&mut self, pick: &crate::widgets::popup::Pick, cx: &mut Context<Self>) {
        let slow = &mut self.slow;
        match pick.field {
            "slow-from-hour" => native::time_picked(&mut slow.from, true, pick.index),
            "slow-from-minute" => native::time_picked(&mut slow.from, false, pick.index),
            "slow-to-hour" => native::time_picked(&mut slow.to, true, pick.index),
            "slow-to-minute" => native::time_picked(&mut slow.to, false, pick.index),
            _ => return,
        }
        cx.notify();
    }
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
        self.slow.limit = number(&self.slow_limit, 1..=10_000_000, now.slow_hours.limit, cx);
        if let Some(why) = dm_desktop_base::scheduler::slow_hours_problem(&self.slow) {
            self.says = Some(why.into());
            cx.notify();
            return;
        }
        let mut wanted = now.clone();
        wanted.advanced.speed_limit =
            number(&self.limit, 0..=1024 * 1024, now.advanced.speed_limit, cx);
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
        self.screen.begin_in(window);
        let screen = self.screen.clone();
        let slow_on = self.slow.enabled;
        let labels = labels();
        let width = native::label_width(&labels);
        let number_field = |input: &Entity<InputState>, name: &'static str| {
            div()
                .w(px(crate::widgets::native::NUMBER_FIELD))
                .child(native::field(input, name, theme))
        };
        let root = crate::widgets::native::dialog("limiter", t("Speed limiter"), theme, cx)
            .on_action(
                cx.listener(|this, pick: &crate::widgets::popup::Pick, _, cx| this.pick(pick, cx)),
            )
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter"
                    && crate::widgets::native::return_is_default(window, Some(&this.focus), cx)
                {
                    this.ok(window, cx);
                }
            }))
            .child(crate::widgets::fit::fitted(
                native::body(theme)
                    .child(native::row_short(
                        Some(labels[0]),
                        width,
                        &screen,
                        number_field(&self.limit, "Speed limit (KB/s, 0 for none)"),
                    ))
                    // Download limits, for a link with a fair-use allowance:
                    // downloads are held once it is used and go on after.
                    .child(native::row_short(
                        Some(labels[1]),
                        width,
                        &screen,
                        number_field(&self.mb, "Data allowance (MB, 0 for none)"),
                    ))
                    .child(native::row_short(
                        Some(labels[2]),
                        width,
                        &screen,
                        number_field(&self.hours, "Allowance period (hours)"),
                    ))
                    // A lower limit for set hours of every day.
                    .child(native::row(
                        None,
                        width,
                        &screen,
                        native::switch_inline(
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
                    // The slower hours on the form's own label column (gaps row E32), greyed
                    // and taking no clicks while they are off: a second column of labels
                    // nested in the first had them begin at two places.
                    .child(native::enabled(
                        slow_on,
                        native::row_short(
                            Some(labels[3]),
                            width,
                            &screen,
                            native::time_picker(
                                ("slow-from-hour", "slow-from-minute"),
                                &self.slow.from,
                                theme,
                                &screen,
                            ),
                        ),
                    ))
                    .child(native::enabled(
                        slow_on,
                        native::row_short(
                            Some(labels[4]),
                            width,
                            &screen,
                            native::time_picker(
                                ("slow-to-hour", "slow-to-minute"),
                                &self.slow.to,
                                theme,
                                &screen,
                            ),
                        ),
                    ))
                    .child(native::enabled(
                        slow_on,
                        native::row_short(
                            Some(labels[5]),
                            width,
                            &screen,
                            number_field(&self.slow_limit, "Speed limit in those hours (KB/s)"),
                        ),
                    ))
                    .child(native::says(
                        "limiter-says",
                        self.says.clone(),
                        theme,
                        &screen,
                    ))
                    .child(native::buttons(
                        // Its page of the guide (gaps row E11).
                        vec![
                            native::help_button("speed-limiter", theme, &screen).into_any_element()
                        ],
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            native::button("ok", t("Save").into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx)))
                                .into_any_element(),
                        ],
                    )),
            ));
        crate::window::safe::kept_clear(root, window)
    }
}

/// The form's labels, in the language in force.
fn labels() -> [&'static str; 6] {
    [
        t("Speed limit (KB/s, 0 for none)"),
        t("Data allowance (MB, 0 for none)"),
        t("Allowance period (hours)"),
        t("Start at"),
        t("Stop at"),
        t("Speed limit in those hours (KB/s)"),
    ]
}

/// The window's width, in points of dm's design: the label column and the
/// widest control beside it, the slower hours' two rows included, measured
/// as `native::label_width` measures words, with the toggle's own width. At a fixed 560 those rows would
/// run past the right edge in English, Stop at's field and the speed in
/// those hours out of sight.
fn width() -> f32 {
    // Measured as the system sets them, as the labels' column is: a guess at 0.55 of the size
    // a letter fell short, and the speed in those hours ran past the right edge (gaps row V15).
    let words = native::text_width;
    let gap = native::LABEL_GAP;
    // Every row's control beside the one column of labels: a time, a number, the switch.
    let picker = 70.0 + gap + 8.0 + gap + 70.0;
    // The switch's own room before its words: at 24 the German ran to the window's edge.
    let slower = native::SWITCH_INLINE_BEFORE + words(t("Slower between set hours"));
    let controls = picker.max(slower).max(native::NUMBER_FIELD);
    (2.0 * native::MARGIN + native::label_width(&labels()) + gap + controls)
        .max(560.0)
        .ceil()
}

/// Opens the Speed limiter.
pub fn open(view: WeakEntity<Downloads>, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, cx));
}

fn open_now(view: WeakEntity<Downloads>, cx: &mut App) {
    // Asked for again while open: the one open, as it stands.
    if crate::window::screen::bring_named(t("Speed limiter"), cx) {
        return;
    }
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let settings = downloads.read(cx).backend.settings();
    let bounds = Bounds::centered(None, size(px(width()), px(300.0)), cx);
    let opened = crate::window::screen::open_window(
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
            let limit = field(settings.advanced.speed_limit.to_string(), window, cx);
            let mb = field(settings.limit_mb.to_string(), window, cx);
            let hours = field(settings.limit_hours.to_string(), window, cx);
            let slow_limit = field(settings.slow_hours.limit.to_string(), window, cx);
            // The limit takes the keyboard as the dialog opens, selected.
            limit.update(cx, |l, cx| {
                l.focus(window, cx);
                l.select_all(window, cx);
            });
            let limiter = cx.new(|cx| {
                // Return in a field is OK.
                for input in [&limit, &mb, &hours, &slow_limit] {
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
                    slow_limit,
                    says: None,
                    done: false,
                    focus: cx.focus_handle(),
                    screen,
                }
            });
            crate::widgets::native::close_is_cancel(&limiter, window, cx);
            cx.new(|cx| gpui_component::Root::new(limiter, window, cx))
        },
    );
    if let Err(e) = opened {
        dm_core::note!(error, "dm-gpui: the Speed limiter could not be opened: {e}");
    }
}

impl crate::widgets::native::CloseButton for Limiter {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
