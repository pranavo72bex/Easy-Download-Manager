// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! About EDM, centred as the system's own About panels are: the name, the
//! version, what EDM is, the project's pages; Check for updates and what
//! came of it, What’s new and Download the update. Help's Tip of the day.
//! The update downloaded and checked against its release's digest is
//! offered above the list: Install now, or Later. The check is the
//! application's (`edm_shell::app::update`), its words EDM's own, kept with no
//! toolkit (`edm_desktop_base::project`).

use std::sync::{Arc, Mutex};

use edm_core::{AddOptions, State};
use edm_desktop_base::i18n::{t, tf};
use edm_desktop_base::project;
use edm_shell::app::update::Update;
use gpui::{
    div, prelude::*, size, App, Bounds, Context, KeyDownEvent, Role, SharedString, WeakEntity,
    Window, WindowBounds, WindowOptions,
};

use crate::widgets::native;
use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// What the update check has found, shared by the main window and About.
pub type Checked = Arc<Mutex<Update>>;

/// About EDM's view.
pub struct About {
    view: WeakEntity<Downloads>,
    checked: Checked,
    focus: gpui::FocusHandle,
    screen: Screen,
}

impl About {
    fn open_page(&mut self, page: &str, cx: &mut Context<Self>) {
        let page = page.to_string();
        let _ = self.view.update(cx, |view, cx| {
            if let Err(e) = view.backend.platform.open(&page) {
                view.say(e);
                cx.notify();
            }
        });
    }
}

impl Render for About {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin_in(window);
        let screen = self.screen.clone();
        let state = edm_core::lock(&self.checked).clone();
        let notes = matches!(state, Update::Newer(..))
            .then(|| edm_core::lock(&edm_shell::app::update::LATEST_NOTES).clone())
            .flatten();
        let installer = matches!(state, Update::Newer(_, Some(_)));
        let links = [
            (t("Source code"), project::SOURCE),
            (t("Licence (GPL v3)"), project::LICENCE),
            (t("Privacy"), project::PRIVACY),
            (t("Code signing policy"), project::SIGNING),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (words, page))| {
            div()
                .id(("page", i))
                .map(|c| crate::widgets::native::ringed(c, theme))
                .role(Role::Link)
                .aria_label(SharedString::from(words))
                .text_color(theme.accent_text)
                .cursor_pointer()
                .child(screen.label(words))
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(SharedString::from(page))
                        .build(window, cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| this.open_page(page, cx)))
        });
        let root = div()
            .id("about")
            .role(Role::Dialog)
            .map(crate::window::mirror::text)
            .aria_label(t("About EDM"))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, window, _| {
                if event.keystroke.key == "escape" {
                    window.remove_window();
                }
            }))
            .size_full()
            .bg(theme.window);
        let root =
            native::phone_framed(root, t("About EDM"), theme).child(crate::widgets::fit::fitted(
                native::body(theme)
                    .items_center()
                    .child(
                        div()
                            .text_size(px(crate::widgets::native::TITLE_TEXT))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(screen.label("EDM")),
                    )
                    .child(native::note(
                        tf("Version {}", &[&env!("CARGO_PKG_VERSION")]),
                        theme.muted,
                        &screen,
                    ))
                    .child(screen.label(t("A free download manager, written in Rust.")))
                    .child(link_rows(
                        links.map(IntoElement::into_any_element).collect(),
                    ))
                    .child(
                        div()
                            .w_full()
                            .h(px(space::LINE))
                            .my(px(space::XS))
                            .bg(theme.separator),
                    )
                    .child(
                        native::button(
                            "check",
                            t("Check for updates").into(),
                            false,
                            theme,
                            &screen,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            let _ = this.view.update(cx, |view, cx| view.check_for_updates(cx));
                            cx.notify();
                        })),
                    )
                    .child(native::note(
                        project::update_words(&state, env!("CARGO_PKG_VERSION")),
                        theme.muted,
                        &screen,
                    ))
                    .when(notes.is_some() || installer, |d| {
                        d.child(
                            div()
                                .flex()
                                .gap(px(native::BUTTON_GAP))
                                // What changed in it, on the release's own page.
                                .when_some(notes, |d, notes| {
                                    d.child(
                                        native::button(
                                            "whats-new",
                                            t("What’s new").into(),
                                            false,
                                            theme,
                                            &screen,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.open_page(&notes, cx)
                                            }),
                                        ),
                                    )
                                })
                                .when(installer, |d| {
                                    d.child(
                                        native::button(
                                            "download-update",
                                            t("Download the update").into(),
                                            true,
                                            theme,
                                            &screen,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                let _ = this.view.update(cx, |view, cx| {
                                                    view.download_update(cx)
                                                });
                                            }),
                                        ),
                                    )
                                }),
                        )
                    }),
            ));
        crate::window::safe::kept_clear(root, window)
    }
}

/// Tip of the day: one tip, Next tip and Close.
pub struct Tip {
    at: usize,
    focus: gpui::FocusHandle,
    screen: Screen,
}

impl Render for Tip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin_in(window);
        let screen = self.screen.clone();
        let tips = project::tips();
        let root = div()
            .id("tip")
            .role(Role::Dialog)
            .aria_label(t("Tip of the day"))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => window.remove_window(),
                    "enter" => {
                        this.at += 1;
                        cx.notify();
                    }
                    _ => {}
                }
            }))
            .size_full()
            .bg(theme.window)
            .child(crate::widgets::fit::fitted(
                native::body(theme)
                    .child(
                        div()
                            .max_w(px(420.0))
                            .child(screen.label(tips[self.at % tips.len()])),
                    )
                    .child(native::buttons(
                        Vec::new(),
                        vec![
                            native::button("close", t("Close").into(), false, theme, &screen)
                                .on_click(|_, window, _| window.remove_window())
                                .into_any_element(),
                            native::button("next", t("Next tip").into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.at += 1;
                                    cx.notify();
                                }))
                                .into_any_element(),
                        ],
                    )),
            ));
        crate::window::safe::kept_clear(root, window)
    }
}

/// Opens a small window titled `title`, as tall as it turns out, its view
/// built by `build` with the window's keyboard handle.
fn open_small<V: Render + 'static>(
    title: &'static str,
    width: f32,
    cx: &mut App,
    build: impl FnOnce(gpui::FocusHandle, Screen) -> V + 'static,
) -> Option<gpui::AnyWindowHandle> {
    let bounds = Bounds::centered(None, size(px(width), px(200.0)), cx);
    let opened = crate::window::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t(title).into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            let view = cx.new(|_| build(focus, screen));
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        },
    );
    match opened {
        Ok(handle) => Some(handle.into()),
        Err(e) => {
            edm_core::note!(error, "edm-gpui: {title} could not be opened: {e}");
            None
        }
    }
}

impl Downloads {
    /// About EDM (`check`: Check for updates, which opens it and asks at
    /// once); brought forward where it is open.
    pub(crate) fn open_about(&mut self, check: bool, cx: &mut Context<Self>) {
        if check {
            self.check_for_updates(cx);
        }
        let open = cx.windows();
        if let Some(about) = self.windows.about_window.filter(|w| open.contains(w)) {
            let _ = about.update(cx, |_, window, _| window.activate_window());
            return;
        }
        let view = cx.entity().downgrade();
        let checked = self.threads.checked.clone();
        let me = cx.entity().downgrade();
        cx.defer(move |cx| {
            let handle = open_small("About EDM", 420.0, cx, move |focus, screen| About {
                view,
                checked,
                focus,
                screen,
            });
            let _ = me.update(cx, |me, _| me.windows.about_window = handle);
        });
    }

    /// Asks whether a newer EDM is out, and draws About again once the
    /// answer is in.
    pub(crate) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        let (tell, told) = async_channel::bounded::<()>(1);
        edm_shell::app::update::check(
            &self.backend,
            env!("CARGO_PKG_VERSION"),
            self.threads.checked.clone(),
            move || {
                let _ = tell.try_send(());
            },
        );
        cx.spawn(async move |this, cx| {
            let _ = told.recv().await;
            let _ = this.update(cx, |this, cx| {
                this.refresh_about(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// About drawn again, where it is open.
    fn refresh_about(&self, cx: &mut Context<Self>) {
        if let Some(about) = self.windows.about_window {
            let _ = about.update(cx, |_, window, _| window.refresh());
        }
    }

    /// Download the update: the installer the release names, checked
    /// against the digest it published (one that does not match fails and
    /// is removed); offered to install once verified.
    pub(crate) fn download_update(&mut self, cx: &mut Context<Self>) {
        let Update::Newer(_, Some(installer)) = edm_core::lock(&self.threads.checked).clone()
        else {
            return;
        };
        let with = AddOptions {
            checksum: edm_core::Checksum::parse(&format!("sha256:{}", installer.sha256)),
            ..AddOptions::default()
        };
        match self.backend.add_with(&installer.url, with) {
            Ok(id) => {
                self.threads.update_row = Some(id);
                self.say(
                    t("Downloading the update. EDM will offer to install it when it’s verified.")
                        .into(),
                );
            }
            Err(e) => self.say(e),
        }
        self.read();
        cx.notify();
    }

    /// The weekly update check: about once a week, where turned on, a look
    /// for a newer EDM, and About opened to offer it when there is one.
    /// Nothing is installed without the person.
    pub(crate) fn weekly_update(&mut self, cx: &mut Context<Self>) {
        // Due again a week after the last, however long EDM has been running:
        // the time of the last check is what decides.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if let Some(wanted) = edm_shell::app::update::weekly(&self.backend.settings(), now) {
            self.threads.weekly_pending = true;
            self.check_for_updates(cx);
            if let Err(e) = self.backend.set_settings(wanted) {
                self.say(e);
            }
        }
        // A YouTube player whose checks were not answered: fresh site rules asked for at once,
        // once for each such player, where the person lets EDM look for updates (gaps row Y3).
        if self.backend.settings().general.check_updates
            && edm_core::youtube::take_unsolved_player().is_some()
        {
            edm_shell::app::update::fresh_rules(&self.backend);
        }
        if self.threads.weekly_pending
            && matches!(*edm_core::lock(&self.threads.checked), Update::Newer(..))
        {
            self.threads.weekly_pending = false;
            self.open_about(false, cx);
        }
    }

    /// EDM's update, downloaded and checked against its release's digest:
    /// installed from here, EDM closing so the installer can replace it. An
    /// AppImage is not installed but put in place of this one by the
    /// person, so its folder is shown instead.
    pub(crate) fn update_bar(
        &mut self,
        theme: Theme,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let id = self.threads.update_row?;
        // Not in the list yet, the moment it is added: waited for.
        let row = self.rows.iter().find(|r| r.id == id)?;
        match row.state {
            State::Completed if row.verified && !row.path.is_empty() => {}
            // Its own row says why it failed; nothing to offer.
            State::Failed(_) | State::Cancelled => {
                self.threads.update_row = None;
                return None;
            }
            _ => return None,
        }
        let path = row.path.clone();
        let appimage = cfg!(target_os = "linux");
        let (words, default) = if appimage {
            (t("Open folder"), "open-folder")
        } else {
            (t("Install now"), "install")
        };
        Some(
            crate::window::bars::body()
                .child(crate::window::bars::words(
                    screen.label(t("The EDM update is downloaded and checked.")),
                ))
                .child(crate::window::bars::actions().child(
                    native::button("update-later", t("Later").into(), false, theme, screen)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.threads.update_row = None;
                            cx.notify();
                        })),
                )
                .child(
                    native::button(default, words.into(), true, theme, screen).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if appimage {
                                this.reveal_path(&path);
                                this.threads.update_row = None;
                            } else {
                                // A script's click a moment ago does not run
                                // the installer.
                                if this.port.scripted.by_script() {
                                    this.say(
                                        t("Not installed: a script was driving the window a moment ago; click again")
                                            .to_string(),
                                    );
                                    cx.notify();
                                    return;
                                }
                                this.open_path(&path);
                                this.threads.update_row = None;
                                this.ask_quit(cx);
                            }
                            cx.notify();
                        }),
                    ),
                )),
        )
    }

    /// Help's Tip of the day, from the first tip; brought forward where it
    /// is open.
    pub(crate) fn tip_of_the_day(&mut self, cx: &mut Context<Self>) {
        let open = cx.windows();
        if let Some(tip) = self.windows.tip_window.filter(|w| open.contains(w)) {
            let _ = tip.update(cx, |_, window, _| window.activate_window());
            return;
        }
        let me = cx.entity().downgrade();
        cx.defer(move |cx| {
            let handle = open_small("Tip of the day", 460.0, cx, |focus, screen| Tip {
                at: 0,
                focus,
                screen,
            });
            let _ = me.update(cx, |me, _| me.windows.tip_window = handle);
        });
    }
}

/// About's links in two rows, balanced: wrapped as they came, the fourth stood alone on a line
/// of its own (seen in ui-shot.py's picture).
fn link_rows(mut links: Vec<gpui::AnyElement>) -> gpui::Div {
    let second = links.split_off(links.len().div_ceil(2));
    let row = |links: Vec<gpui::AnyElement>| {
        div()
            .flex()
            .flex_wrap()
            .justify_center()
            .gap(px(native::BUTTON_GAP))
            .children(links)
    };
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(space::XS))
        .child(row(links))
        .child(row(second))
}
