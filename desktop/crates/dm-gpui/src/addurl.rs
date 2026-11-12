// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Add URL, a window of its own (B42: work, not a question), with the egui
//! window's words: the address (a link on the clipboard put in, as the
//! egui window does), a sign-in where it needs one, OK and Cancel. What OK
//! does is dm's (`dm_desktop_base::add_url::decide`, B01 and B02); where
//! it sends the address is the main window's (`Downloads::after_add_url`).

use dm_desktop_base::add_url::{decide, Next};
use dm_desktop_base::i18n::t;
use gpui::{
    div, prelude::*, size, App, Bounds, Context, Entity, KeyDownEvent, SharedString, WeakEntity,
    Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::list::Downloads;
use crate::native;
use crate::popup::{self, Pick};
use crate::screen::{Screen, Screens};
use crate::theme::Theme;
use crate::zoom::px;

/// The Add URL window's view.
pub struct AddUrl {
    view: WeakEntity<Downloads>,
    address: Entity<InputState>,
    login: Entity<InputState>,
    password: Entity<InputState>,
    authorize: bool,
    says: Option<SharedString>,
    /// Addresses added here before, to pick again.
    recent: Vec<String>,
    /// Answered: a second Return in the same moment adds nothing.
    done: bool,
    screen: Screen,
}

impl AddUrl {
    /// OK: the address decided on, sent on, or the reason it stays said.
    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        let typed = self.address.read(cx).value().to_string();
        let login = self.login.read(cx).value().to_string();
        let password = self.password.read(cx).value().to_string();
        let Some(view) = self.view.upgrade() else {
            window.remove_window();
            return;
        };
        let (settings, rows) = {
            let view = view.read(cx);
            (view.backend.settings(), view.rows.clone())
        };
        let decided = decide(&typed, self.authorize, &login, &password, &settings, &rows);
        if let Next::Says(words) = decided.next {
            self.says = Some(words.into());
            // The address keeps the keyboard, to be put right, chosen: what is typed next
            // replaces it, as in the egui window.
            self.address.update(cx, |a, cx| {
                a.focus(window, cx);
                a.select_all(window, cx);
            });
            cx.notify();
            return;
        }
        self.done = true;
        window.remove_window();
        let _ = self
            .view
            .update(cx, |view, cx| view.after_add_url(decided, cx));
    }

    /// Cancel, Escape or the close button: nothing added.
    fn cancel(&mut self, window: &mut Window) {
        self.done = true;
        window.remove_window();
    }

    /// An address picked from Recent.
    fn pick(&mut self, pick: &Pick, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(url) = self.recent.get(pick.index).cloned() {
            self.address
                .update(cx, |a, cx| a.set_value(url, window, cx));
            self.says = None;
            cx.notify();
        }
    }

    /// Clear history: the Recent list forgotten.
    fn forget(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.view.upgrade() else {
            return;
        };
        match view.read(cx).backend.remember_urls(Vec::new()) {
            Ok(_) => self.recent.clear(),
            Err(e) => self.says = Some(e.into()),
        }
        cx.notify();
    }
}

impl Render for AddUrl {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let authorize = self.authorize;
        // A form as macOS lays one out: the labels right-aligned in a
        // column of their own, every control in a second.
        let labels = [t("Address"), t("Recent"), t("Login"), t("Password")];
        let width = native::label_width(&labels);
        let recent = (!self.recent.is_empty()).then(|| {
            let shown: Vec<SharedString> = self
                .recent
                .iter()
                .map(|u| dm_desktop_base::format::shown_url(u).into())
                .collect();
            // Addresses added here before, to pick again, and a way to
            // forget them.
            native::row(
                Some(t("Recent")),
                width,
                &screen,
                div()
                    .flex()
                    .items_center()
                    .gap(px(native::BUTTON_GAP))
                    .child(popup::button(
                        "recent",
                        SharedString::default(),
                        shown,
                        None,
                        theme,
                        &screen,
                    ))
                    .child(
                        native::button("forget", t("Clear history").into(), false, theme, &screen)
                            .on_click(cx.listener(|this, _, _, cx| this.forget(cx))),
                    ),
            )
        });
        crate::native::dialog("add-url", t("Add URL"), theme, cx)
            .on_action(cx.listener(|this, pick: &Pick, window, cx| this.pick(pick, window, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter" {
                    this.ok(window, cx);
                }
            }))
            .child(crate::fit::fitted(
                native::body(theme)
                    .child(native::row(
                        Some(t("Address")),
                        width,
                        &screen,
                        native::field(&self.address, "Address", theme),
                    ))
                    .children(recent)
                    .child(native::row(
                        None,
                        width,
                        &screen,
                        native::checkbox(
                            "authorize",
                            t("Needs a sign-in").into(),
                            authorize,
                            theme,
                            &screen,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.authorize = !this.authorize;
                            cx.notify();
                        })),
                    ))
                    .when(authorize, |d| {
                        d.child(native::row(
                            Some(t("Login")),
                            width,
                            &screen,
                            native::field(&self.login, "Login", theme),
                        ))
                        .child(native::row(
                            Some(t("Password")),
                            width,
                            &screen,
                            native::field(&self.password, "Password", theme),
                        ))
                    })
                    .when_some(self.says.clone(), |d, says| {
                        d.child(native::row(
                            None,
                            width,
                            &screen,
                            native::note(says, theme.warning, &screen),
                        ))
                    })
                    // The default rightmost, Cancel beside it, as on a Mac.
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

/// Opens Add URL over the middle of the screen, a link on the clipboard
/// put in its address.
pub fn open(view: WeakEntity<Downloads>, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, cx));
}

fn open_now(view: WeakEntity<Downloads>, cx: &mut App) {
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let recent = dm_desktop_base::add_url::recent(&downloads.read(cx).backend.settings());
    let clipboard = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .and_then(|text| dm_shell::app::links::looks_downloadable(&text))
        .unwrap_or_default();
    let bounds = Bounds::centered(None, size(px(520.0), px(220.0)), cx);
    let opened = crate::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Add URL").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let address = cx.new(|cx| {
                let mut a = InputState::new(window, cx).placeholder("https://");
                if !clipboard.is_empty() {
                    a.set_value(clipboard.clone(), window, cx);
                }
                a
            });
            let login = cx.new(|cx| InputState::new(window, cx));
            let password = cx.new(|cx| InputState::new(window, cx).masked(true));
            // The link it was opened with is selected, so a link pasted or
            // typed replaces it rather than joining it.
            address.update(cx, |a, cx| {
                a.focus(window, cx);
                a.select_all(window, cx);
            });
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let add = cx.new(|cx| {
                // Return in a field is OK; what was said of the last address
                // goes when it is changed (the error stayed under a good one).
                for input in [&address, &login, &password] {
                    cx.subscribe_in(input, window, |this: &mut AddUrl, _, event, window, cx| {
                        match event {
                            InputEvent::PressEnter { .. } => this.ok(window, cx),
                            InputEvent::Change if this.says.take().is_some() => cx.notify(),
                            _ => {}
                        }
                    })
                    .detach();
                }
                AddUrl {
                    view,
                    address: address.clone(),
                    login,
                    password,
                    authorize: false,
                    says: None,
                    recent,
                    done: false,
                    screen,
                }
            });
            crate::native::close_is_cancel(&add, window, cx);
            cx.new(|cx| gpui_component::Root::new(add, window, cx))
        },
    );
    if let Err(e) = opened {
        eprintln!("dm-gpui: Add URL could not be opened: {e}");
    }
}

impl crate::native::CloseButton for AddUrl {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
