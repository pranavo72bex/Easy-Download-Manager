// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Add numbered series, a window of its own, with the egui window's words:
//! an address with a * where the number or letter changes, the range and
//! how many digits each number has; every address the series makes listed
//! before anything is fetched; Start now and Add for later. The series is
//! dm's (`dm_shell::lists::series`), the reasons it cannot be made too
//! (`dm_desktop_base::project::series_error`).

use dm_core::{AddOptions, MAIN_QUEUE};
use dm_desktop_base::i18n::{t, tn};
use gpui::{
    div, prelude::*, px, size, uniform_list, App, Bounds, Context, Entity, KeyDownEvent, Role,
    SharedString, WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::list::Downloads;
use crate::native;
use crate::popup::{self, Pick};
use crate::screen::{Screen, Screens};
use crate::theme::Theme;

/// How many of the series' addresses show at once; the rest scroll.
const SHOWN: usize = 6;
/// One listed address's height.
const LINE: f32 = 16.0;

/// The window's view.
pub struct Series {
    view: WeakEntity<Downloads>,
    template: Entity<InputState>,
    from: Entity<InputState>,
    to: Entity<InputState>,
    /// How many digits each number is padded to; 0 leaves it as it is.
    digits: usize,
    /// What a press of Start now or Add for later said; it stands only
    /// until the address or the range is changed.
    says: Option<SharedString>,
    done: bool,
    screen: Screen,
}

impl Series {
    /// The series as typed: its addresses, or why there are none.
    fn made(&self, cx: &App) -> Result<Vec<String>, dm_shell::lists::SeriesError> {
        dm_shell::lists::series(
            &self.template.read(cx).value(),
            &self.from.read(cx).value(),
            &self.to.read(cx).value(),
            self.digits,
        )
    }

    /// Start now, or Add for later (`later`): the series added.
    fn add(&mut self, later: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.done || self.template.read(cx).value().trim().is_empty() {
            return;
        }
        let urls = match self.made(cx) {
            Ok(urls) => urls,
            Err(e) => {
                self.says = Some(dm_desktop_base::project::series_error(e).into());
                cx.notify();
                return;
            }
        };
        let with = AddOptions {
            later,
            queue: if later {
                MAIN_QUEUE.into()
            } else {
                String::new()
            },
            ..AddOptions::default()
        };
        let Some(view) = self.view.upgrade() else {
            window.remove_window();
            return;
        };
        match view.update(cx, |view, cx| view.add(urls, with, cx)) {
            Ok(n) => {
                self.done = true;
                window.remove_window();
                view.update(cx, |view, cx| {
                    view.say(tn(
                        "Added {} download.||Added {} downloads.",
                        n as u64,
                        &[&n],
                    ));
                    cx.notify();
                });
            }
            Err(e) => {
                self.says = Some(e.into());
                cx.notify();
            }
        }
    }

    fn cancel(&mut self, window: &mut Window) {
        self.done = true;
        window.remove_window();
    }

    /// Digits picked.
    fn pick(&mut self, pick: &Pick, cx: &mut Context<Self>) {
        if pick.field == "digits" {
            self.digits = pick.index;
            self.says = None;
            cx.notify();
        }
    }
}

impl Render for Series {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let template = self.template.read(cx).value().to_string();
        let typed = !template.trim().is_empty()
            && !self.from.read(cx).value().trim().is_empty()
            && !self.to.read(cx).value().trim().is_empty();
        // One line, its room kept whether or not it says anything, so the
        // window does not jump as it starts to; nothing said of the range
        // until both its ends are typed.
        let made = self.made(cx);
        let (line, warning): (String, bool) = match (&self.says, &made) {
            (Some(says), _) => (says.to_string(), true),
            (None, Ok(urls)) => match urls.first() {
                Some(first) => (
                    tn(
                        "{} address: {}||{} addresses, the first {}",
                        urls.len() as u64,
                        &[&urls.len(), first],
                    ),
                    false,
                ),
                None => (String::new(), false),
            },
            (None, Err(e)) if typed => (dm_desktop_base::project::series_error(*e).into(), true),
            (None, Err(_)) => (String::new(), false),
        };
        let listed = made.unwrap_or_default();
        let addressed = !template.trim().is_empty();
        let labels = [t("Address with *"), t("From")];
        let width = native::label_width(&labels);
        let digits: Vec<SharedString> = (0..=9).map(|d| d.to_string().into()).collect();
        let count = listed.len();
        div()
            .id("series")
            .role(Role::Dialog)
            .aria_label(t("Add numbered series"))
            .on_action(cx.listener(|this, pick: &Pick, _, cx| this.pick(pick, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, _| {
                if event.keystroke.key == "escape" {
                    this.cancel(window);
                }
            }))
            .size_full()
            .bg(theme.window)
            .child(crate::fit::fitted(
                native::body(theme)
                    .child(screen.label(t(
                        "Put * where the number or letter changes, as in example.com/photo*.jpg",
                    )))
                    .child(native::row(
                        Some(labels[0]),
                        width,
                        &screen,
                        native::field(&self.template, "Address with *", theme),
                    ))
                    .child(native::row(
                        Some(labels[1]),
                        width,
                        &screen,
                        div()
                            .flex()
                            .items_center()
                            .gap(px(native::LABEL_GAP))
                            .child(div().w(px(70.0)).child(native::field(&self.from, "From", theme)))
                            .child(screen.label(t("To")))
                            .child(div().w(px(70.0)).child(native::field(&self.to, "To", theme)))
                            .child(
                                div()
                                    .id("digits-label")
                                    .child(screen.label(t("Digits")))
                                    .tooltip(|window, cx| {
                                        gpui_component::tooltip::Tooltip::new(SharedString::from(t(
                                            "How many digits each number has: 3 makes 7 into 007; 0 leaves it as it is",
                                        )))
                                        .build(window, cx)
                                    }),
                            )
                            .child(popup::button(
                                "digits",
                                self.digits.to_string().into(),
                                digits,
                                Some(self.digits),
                                theme,
                                &screen,
                            )),
                    ))
                    .child(
                        div()
                            .h(px(native::SMALL_TEXT + 6.0))
                            .child(native::note(
                                line,
                                if warning { theme.warning } else { theme.muted },
                                &screen,
                            )),
                    )
                    // Every address the series makes, so a wrong padding or
                    // end is seen before anything is fetched; its room kept
                    // whether or not there is a list yet, only the rows in
                    // view drawn.
                    .child(
                        div()
                            .h(px(LINE * SHOWN as f32 + 8.0))
                            .rounded(px(native::RADIUS))
                            .border_1()
                            .border_color(theme.separator)
                            .bg(theme.background)
                            .p(px(4.0))
                            .when(listed.is_empty(), |d| {
                                d.child(native::note(
                                    t("Each address the series makes is listed here."),
                                    theme.muted,
                                    &screen,
                                ))
                            })
                            .when(!listed.is_empty(), |d| {
                                let screen = screen.clone();
                                d.child(
                                    uniform_list("series-addresses", count, move |range, _, cx| {
                                        let mono = crate::native::mono(cx);
                                        range
                                            .map(|i| {
                                                div()
                                                    .h(px(LINE))
                                                    .w_full()
                                                    .font_family(mono.clone())
                                                    .text_size(px(native::SMALL_TEXT))
                                                    .whitespace_nowrap()
                                                    .overflow_hidden()
                                                    .text_ellipsis()
                                                    .child(screen.label(listed[i].clone()))
                                            })
                                            .collect()
                                    })
                                    .size_full(),
                                )
                            }),
                    )
                    // Greyed until there is an address to make a series of;
                    // with one, a press says what is wrong with the range.
                    .child(native::buttons(
                        vec![native::enabled(
                            addressed,
                            native::button(
                                "later",
                                t("Add for later").into(),
                                false,
                                theme,
                                &screen,
                            )
                            .on_click(cx.listener(|this, _, window, cx| this.add(true, window, cx))),
                        )
                        .into_any_element()],
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            native::enabled(
                                addressed,
                                native::button("start", t("Start now").into(), true, theme, &screen)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.add(false, window, cx)),
                                    ),
                            )
                            .into_any_element(),
                        ],
                    )),
            ))
    }
}

/// Opens Add numbered series.
pub fn open(view: WeakEntity<Downloads>, cx: &mut App) {
    cx.defer(move |cx| {
        let bounds = Bounds::centered(None, size(px(520.0), px(300.0)), cx);
        let opened = crate::screen::open_window(
            cx,
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(t("Add numbered series").into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                let screen = Screen::default();
                Screens::register(window.window_handle(), screen.clone(), true, cx);
                let template = cx.new(|cx| InputState::new(window, cx).placeholder("https://"));
                let from = cx.new(|cx| InputState::new(window, cx));
                let to = cx.new(|cx| InputState::new(window, cx));
                template.update(cx, |t, cx| t.focus(window, cx));
                let series = cx.new(|cx| {
                    // Return is Start now; a change takes back what a press
                    // said, and the count and list follow what is typed.
                    for input in [&template, &from, &to] {
                        cx.subscribe_in(
                            input,
                            window,
                            |this: &mut Series, _, event, window, cx| match event {
                                InputEvent::PressEnter { .. } => this.add(false, window, cx),
                                InputEvent::Change => {
                                    this.says = None;
                                    cx.notify();
                                }
                                _ => {}
                            },
                        )
                        .detach();
                    }
                    Series {
                        view,
                        template,
                        from,
                        to,
                        digits: 0,
                        says: None,
                        done: false,
                        screen,
                    }
                });
                crate::native::close_is_cancel(&series, window, cx);
                cx.new(|cx| gpui_component::Root::new(series, window, cx))
            },
        );
        if let Err(e) = opened {
            eprintln!("dm-gpui: Add numbered series could not be opened: {e}");
        }
    });
}

impl crate::native::CloseButton for Series {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
