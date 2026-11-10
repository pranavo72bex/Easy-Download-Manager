// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Links on the clipboard, a window of its own, with the egui window's
//! words: every link the clipboard holds (`dm_shell::app::links::links_in`),
//! each ticked but one already in the list, which is offered unticked and
//! said to be there; Check all, Uncheck all; Start now and Add for later.

use dm_core::{AddOptions, MAIN_QUEUE};
use dm_desktop_base::i18n::{t, tf, tn, Said};
use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, KeyDownEvent, Role, WeakEntity, Window,
    WindowBounds, WindowOptions,
};

use crate::list::Downloads;
use crate::native;
use crate::screen::{Screen, Screens};
use crate::theme::Theme;

/// The window's view.
pub struct ClipLinks {
    view: WeakEntity<Downloads>,
    /// Each link, whether it is ticked, and whether it is listed already.
    links: Vec<(String, bool, bool)>,
    /// The window's keyboard, so Return and Escape reach it.
    focus: gpui::FocusHandle,
    done: bool,
    screen: Screen,
}

impl ClipLinks {
    /// Start now, or Add for later (`later`): the links ticked added.
    fn add(&mut self, later: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        self.done = true;
        window.remove_window();
        let chosen: Vec<String> = self
            .links
            .iter()
            .filter(|l| l.1)
            .map(|l| l.0.clone())
            .collect();
        let with = AddOptions {
            later,
            queue: if later {
                MAIN_QUEUE.into()
            } else {
                String::new()
            },
            ..AddOptions::default()
        };
        let _ = self.view.update(cx, |view, cx| {
            let said = match view.add(chosen, with, cx) {
                Ok(n) if later => Said::counting_naming(
                    "Added {} download to {}.||Added {} downloads to {}.",
                    n as u64,
                    &[&n, &MAIN_QUEUE],
                )
                .shown(),
                Ok(n) => tn(
                    "Started {} download.||Started {} downloads.",
                    n as u64,
                    &[&n],
                ),
                Err(e) => e,
            };
            view.say(said);
            cx.notify();
        });
    }

    fn cancel(&mut self, window: &mut Window) {
        self.done = true;
        window.remove_window();
    }

    /// Check all, or Uncheck all.
    fn tick_all(&mut self, on: bool, cx: &mut Context<Self>) {
        for link in &mut self.links {
            link.1 = on;
        }
        cx.notify();
    }
}

impl Render for ClipLinks {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let ticked = self.links.iter().filter(|l| l.1).count();
        let found = self.links.len();
        let rows = self
            .links
            .iter()
            .enumerate()
            .map(|(i, (link, on, listed))| {
                let shown = dm_desktop_base::format::shown_url(link);
                let words = if *listed {
                    tf("{} (already in the list)", &[&shown])
                } else {
                    shown
                };
                native::checkbox(("link", i), words.into(), *on, theme, &screen).on_click(
                    cx.listener(move |this, _, _, cx| {
                        if let Some(link) = this.links.get_mut(i) {
                            link.1 = !link.1;
                        }
                        cx.notify();
                    }),
                )
            });
        div()
            .id("clip-links")
            .role(Role::Dialog)
            .aria_label(t("Links on the clipboard"))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => this.cancel(window),
                    "enter" => this.add(false, window, cx),
                    _ => {}
                }
            }))
            .size_full()
            .bg(theme.window)
            .child(crate::fit::fitted(
                native::body(theme)
                    // How many were found, and how many are ticked.
                    .child(screen.label(tn(
                        "Found {} link; {} ticked.||Found {} links; {} ticked.",
                        found as u64,
                        &[&found, &ticked],
                    )))
                    .child(
                        div()
                            .flex()
                            .gap(px(native::LABEL_GAP))
                            .child(
                                native::button(
                                    "check-all",
                                    t("Check all").into(),
                                    false,
                                    theme,
                                    &screen,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.tick_all(true, cx))),
                            )
                            .child(
                                native::button(
                                    "uncheck-all",
                                    t("Uncheck all").into(),
                                    false,
                                    theme,
                                    &screen,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.tick_all(false, cx))),
                            ),
                    )
                    .child(
                        div()
                            .id("links")
                            .max_h(px(260.0))
                            .overflow_y_scroll()
                            .rounded(px(native::RADIUS))
                            .border_1()
                            .border_color(theme.separator)
                            .bg(theme.background)
                            .p(px(6.0))
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .children(rows),
                    )
                    .child(native::buttons(
                        vec![native::button(
                            "later",
                            t("Add for later").into(),
                            false,
                            theme,
                            &screen,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.add(true, window, cx)))
                        .into_any_element()],
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            native::button("start", t("Start now").into(), true, theme, &screen)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.add(false, window, cx)),
                                )
                                .into_any_element(),
                        ],
                    )),
            ))
    }
}

impl Downloads {
    /// Add links from clipboard: the links the clipboard holds offered to
    /// tick, one already in the list unticked and said so (it was fetched
    /// again as `e (1).zip`); none, said.
    pub(crate) fn add_from_clipboard(&mut self, cx: &mut gpui::Context<Self>) {
        let text = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();
        let found = dm_shell::app::links::links_in(&text);
        if found.is_empty() {
            self.say(t("There are no links on the clipboard.").into());
            cx.notify();
            return;
        }
        let listed = dm_shell::app::control_api::listed_addresses(&self.rows);
        let links = found
            .into_iter()
            .map(|l| {
                let there = listed.contains(&dm_shell::lists::bare(&l));
                (l, !there, there)
            })
            .collect();
        open(cx.entity().downgrade(), links, cx);
    }
}

/// Opens Links on the clipboard for `links`.
fn open(view: WeakEntity<Downloads>, links: Vec<(String, bool, bool)>, cx: &mut App) {
    cx.defer(move |cx| {
        let bounds = Bounds::centered(None, size(px(520.0), px(300.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(t("Links on the clipboard").into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                let screen = Screen::default();
                Screens::register(window.window_handle(), screen.clone(), true, cx);
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                let links = cx.new(|_| ClipLinks {
                    view,
                    links,
                    focus,
                    done: false,
                    screen,
                });
                crate::native::close_is_cancel(&links, window, cx);
                cx.new(|cx| gpui_component::Root::new(links, window, cx))
            },
        );
        if let Err(e) = opened {
            eprintln!("dm-gpui: Links on the clipboard could not be opened: {e}");
        }
    });
}

impl crate::native::CloseButton for ClipLinks {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
