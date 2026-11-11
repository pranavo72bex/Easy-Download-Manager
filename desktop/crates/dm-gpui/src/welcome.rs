// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A new install's first window, "Add dm to your browser", as the egui
//! window's: what the extension does, a button for each browser on the
//! computer that opens its extension's page there, where downloads go
//! (confirmed or changed now), and that dm starts with the computer, in the
//! tray; Done, or Later. Once, and only while no browser has spoken to dm;
//! not for a dm on a home of its own (tests and checks set `DM_HOME`).

use dm_desktop_base::i18n::{t, tf};
use dm_shell::register::{Installed, Store};
use gpui::{
    div, prelude::*, size, App, Bounds, Context, Entity, SharedString, WeakEntity, Window,
    WindowBounds, WindowOptions,
};
use gpui_component::input::InputState;

use crate::list::Downloads;
use crate::native;
use crate::screen::{Screen, Screens};
use crate::theme::Theme;
use crate::zoom::px;

/// Written once the welcome has been shown.
const WELCOMED: &str = "welcomed";
/// Written once "Not now" is chosen on the add-to-browser bar.
const BROWSER_HINT_HIDDEN: &str = "browser-hint.hidden";

/// The welcome's view.
pub struct Welcome {
    view: WeakEntity<Downloads>,
    /// The browsers on this computer.
    found: Vec<Installed>,
    folder: Entity<InputState>,
    /// "Start dm with the computer, in the tray".
    at_login: bool,
    focus: gpui::FocusHandle,
    done: bool,
    screen: Screen,
}

/// A browser's extension page.
fn page(store: Store) -> &'static str {
    match store {
        Store::Edge => dm_desktop_base::project::EXTENSION_EDGE,
        Store::Firefox => dm_desktop_base::project::EXTENSION_FIREFOX,
        Store::Chrome => dm_desktop_base::project::EXTENSION_CHROME,
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
            if !folder.is_empty() && chosen != wanted.download_dir {
                wanted.download_dir = chosen;
            }
            wanted.start_on_login = at_login;
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
        crate::services::ask_then_in(
            crate::services::Asked::folder(),
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
        self.screen.begin();
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
        crate::native::dialog("welcome", t("Add dm to your browser"), theme, cx)
            .track_focus(&self.focus)
            .child(crate::fit::fitted(
                native::body(theme)
                    .child(screen.label(t(
                        "Add the extension so downloads from your browser come to dm.",
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
                    // What dm changed on the computer, said where it can be
                    // undone: it starts with the computer, in the tray, so a
                    // download clicked in the browser always has dm to go to.
                    .child(
                        native::checkbox(
                            "at-login",
                            t("Start dm with the computer, in the tray").into(),
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
                    .child(div().pl(px(20.0)).child(native::note(
                        t("So browser downloads always reach dm. Change this later in Options › General."),
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
            ))
    }
}

impl native::CloseButton for Welcome {
    fn close_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish(window, cx);
    }
}

impl Downloads {
    /// A new install's first window, once, and only while no browser has
    /// spoken to dm; not for a dm on a home of its own (tests and checks set
    /// `DM_HOME`).
    pub(crate) fn welcome_if_new(&mut self, cx: &mut Context<Self>) {
        let own_home = std::env::var_os("DM_HOME").is_some_and(|h| !h.is_empty());
        let home = dm_shell::settings::home();
        if self.hidden
            || own_home
            || dm_shell::host::browser_seen(&home)
            || home.join(WELCOMED).exists()
            || home.join(BROWSER_HINT_HIDDEN).exists()
        {
            return;
        }
        self.welcome(cx);
    }

    /// Opens the welcome with the browsers found on this computer.
    pub(crate) fn welcome(&mut self, cx: &mut Context<Self>) {
        let user = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let found = dm_shell::register::installed_browsers(&user);
        let settings = self.backend.settings();
        let home = dm_shell::settings::home();
        let _ = std::fs::create_dir_all(&home);
        let _ = std::fs::write(home.join(WELCOMED), b"");
        let view = cx.entity().downgrade();
        cx.defer(move |cx| open(view, found, settings, cx));
    }
}

fn open(
    view: WeakEntity<Downloads>,
    found: Vec<Installed>,
    settings: dm_shell::Settings,
    cx: &mut App,
) {
    let bounds = Bounds::centered(None, size(px(480.0), px(300.0)), cx);
    let opened = crate::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Add dm to your browser").into()),
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
                f.set_value(settings.download_dir.display().to_string(), window, cx);
                f
            });
            let welcome = cx.new(|_| Welcome {
                view,
                found,
                folder,
                at_login: settings.start_on_login,
                focus,
                done: false,
                screen,
            });
            native::close_is_cancel(&welcome, window, cx);
            cx.new(|cx| gpui_component::Root::new(welcome, window, cx))
        },
    );
    if let Err(e) = opened {
        eprintln!("dm-gpui: the welcome could not be opened: {e}");
    }
}
