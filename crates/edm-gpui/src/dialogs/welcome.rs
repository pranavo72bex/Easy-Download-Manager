// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A new install's first window, "Add EDM to your browser": what the
//! extension does, a button for each browser on the computer that opens its
//! extension's page there, where downloads go (confirmed or changed now),
//! and that EDM starts with the computer, in the tray; Done, or Later. Once,
//! and only while no browser has spoken to EDM; not for an EDM on a home of
//! its own (tests and checks set `EDM_HOME`).

use edm_desktop_base::i18n::{t, tf};
use edm_shell::register::{Installed, Store};
use gpui::{
    div, prelude::*, size, App, Bounds, Context, Entity, SharedString, WeakEntity, Window,
    WindowBounds, WindowOptions,
};
use gpui_component::input::InputState;

use crate::widgets::native;
use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// Written once the welcome has been shown.
const WELCOMED: &str = edm_core::paths::WELCOMED;
/// Written once "Not now" is chosen on the add-to-browser bar.
const BROWSER_HINT_HIDDEN: &str = edm_core::paths::BROWSER_HINT_HIDDEN;

/// The welcome's view.
pub struct Welcome {
    view: WeakEntity<Downloads>,
    /// The browsers on this computer.
    found: Vec<Installed>,
    folder: Entity<InputState>,
    /// "Start EDM when I sign in".
    at_login: bool,
    focus: gpui::FocusHandle,
    done: bool,
    screen: Screen,
}

/// A browser's extension page.
fn page(store: Store) -> &'static str {
    match store {
        Store::Edge => edm_desktop_base::project::EXTENSION_EDGE,
        Store::Firefox => edm_desktop_base::project::EXTENSION_FIREFOX,
        Store::Chrome => edm_desktop_base::project::EXTENSION_CHROME,
    }
}

impl Welcome {
    /// Done or Later: the folder and the login item kept as now shown, and
    /// the window closed.
    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        self.done = true;
        let folder = self.folder.read(cx).value().trim().to_string();
        let at_login = self.at_login;
        let _ = self.view.update(cx, |view, cx| {
            let mut wanted = view.backend.settings();
            let chosen = std::path::PathBuf::from(&folder);
            if !folder.is_empty() && chosen != wanted.folders.download_dir {
                wanted.folders.download_dir = chosen;
            }
            wanted.general.start_on_login = at_login;
            if let Err(e) = view.apply_settings(wanted, cx) {
                view.say(e);
            }
        });
        window.remove_window();
    }

    fn open_page(&mut self, page: &'static str, cx: &mut Context<Self>) {
        let _ = self.view.update(cx, |view, _| view.open_path(page));
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::platform::services::ask_then_in(
            crate::platform::services::Asked::folder(),
            window,
            cx,
            |this: &mut Self, picked, window, cx| {
                this.folder.update(cx, |f, cx| {
                    f.set_value(picked.display().to_string(), window, cx)
                });
            },
        );
    }
}

impl Render for Welcome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin_in(window);
        let screen = self.screen.clone();
        let pages: Vec<(SharedString, &'static str)> = if self.found.is_empty() {
            [
                ("Microsoft Edge".to_string(), Store::Edge),
                ("Firefox".to_string(), Store::Firefox),
                (t("Chrome and others").to_string(), Store::Chrome),
            ]
            .into_iter()
            .map(|(name, store)| (name.into(), page(store)))
            .collect()
        } else {
            self.found
                .iter()
                .map(|b| (tf("Add to {}", &[&b.name]).into(), page(b.store)))
                .collect()
        };
        let none_found = self.found.is_empty();
        let buttons = pages.into_iter().enumerate().map(|(n, (words, page))| {
            native::button(("browser", n), words, false, theme, &screen)
                .on_click(cx.listener(move |this, _, _, cx| this.open_page(page, cx)))
        });
        let root = crate::widgets::native::dialog("welcome", t("Add EDM to your browser"), theme, cx)
            .track_focus(&self.focus)
            .child(crate::widgets::fit::fitted(
                native::body(theme)
                    .child(screen.label(t(
                        "Add the extension so downloads from your browser come to EDM.",
                    )))
                    .when(none_found, |d| {
                        d.child(native::note(
                            t("No browser found. Here is the page for each:"),
                            theme.muted,
                            &screen,
                        ))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(native::LABEL_GAP))
                            .children(buttons),
                    )
                    // Where downloads go, to confirm or change now: the
                    // second of the three steps (the browser, the folder,
                    // done).
                    .child(
                        div()
                            .mt(px(native::ROW_GAP))
                            .child(screen.label(t("Download folder"))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(native::LABEL_GAP))
                            .child(div().flex_1().child(native::field(
                                &self.folder,
                                "Download folder",
                                theme,
                            )))
                            .child(
                                native::button("browse", t("Browse…").into(), false, theme, &screen)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.browse(window, cx)),
                                    ),
                            ),
                    )
                    // What EDM changed on the computer, said where it can be
                    // undone: it starts with the computer, in the tray, so a
                    // download clicked in the browser always has EDM to go to.
                    .child(
                        native::switch_inline(
                            "at-login",
                            t("Start EDM when I sign in").into(),
                            self.at_login,
                            theme,
                            &screen,
                        )
                        .mt(px(native::ROW_GAP))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.at_login = !this.at_login;
                            cx.notify();
                        })),
                    )
                    .child(div().pl(px(space::XXL)).child(native::note(
                        t("So browser downloads always reach EDM. Change this later in Options › General."),
                        theme.muted,
                        &screen,
                    )))
                    .child(native::buttons(
                        Vec::new(),
                        vec![
                            native::button("later", t("Later").into(), false, theme, &screen)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.finish(window, cx)),
                                )
                                .into_any_element(),
                            native::button("done", t("Done").into(), true, theme, &screen)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.finish(window, cx)),
                                )
                                .into_any_element(),
                        ],
                    )),
            ));
        crate::window::safe::kept_clear(root, window)
    }
}

impl native::CloseButton for Welcome {
    fn close_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish(window, cx);
    }
}

impl Downloads {
    /// A new install's first window, once, and only while no browser has
    /// spoken to EDM; not for an EDM on a home of its own (tests and checks set
    /// `EDM_HOME`).
    pub(crate) fn welcome_if_new(&mut self, cx: &mut Context<Self>) {
        let own_home = edm_core::env::get_os("EDM_HOME").is_some_and(|h| !h.is_empty());
        let home = edm_shell::settings::home();
        if self.system.hidden
            || own_home
            || edm_shell::host::browser_seen(&home)
            || home.join(WELCOMED).exists()
            || home.join(BROWSER_HINT_HIDDEN).exists()
        {
            return;
        }
        self.welcome(cx);
    }

    /// Opens the welcome with the browsers found on this computer.
    pub(crate) fn welcome(&mut self, cx: &mut Context<Self>) {
        let user = edm_core::folders::home_or_profile().unwrap_or_default();
        let found = edm_shell::register::installed_browsers(&user);
        let settings = self.backend.settings();
        let home = edm_shell::settings::home();
        let _ = std::fs::create_dir_all(&home);
        let _ = std::fs::write(home.join(WELCOMED), b"");
        let view = cx.entity().downgrade();
        cx.defer(move |cx| open(view, found, settings, cx));
    }
}

fn open(
    view: WeakEntity<Downloads>,
    found: Vec<Installed>,
    settings: edm_shell::Settings,
    cx: &mut App,
) {
    let (width, height) = crate::widgets::native::WindowSize::Welcome.here();
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    let opened = crate::window::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Add EDM to your browser").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            let folder = cx.new(|cx| {
                let mut f = InputState::new(window, cx);
                f.set_value(
                    settings.folders.download_dir.display().to_string(),
                    window,
                    cx,
                );
                f
            });
            let welcome = cx.new(|_| Welcome {
                view,
                found,
                folder,
                at_login: settings.general.start_on_login,
                focus,
                done: false,
                screen,
            });
            native::close_is_cancel(&welcome, window, cx);
            cx.new(|cx| gpui_component::Root::new(welcome, window, cx))
        },
    );
    if let Err(e) = opened {
        edm_core::note!(error, "edm-gpui: the welcome could not be opened: {e}");
    }
}
