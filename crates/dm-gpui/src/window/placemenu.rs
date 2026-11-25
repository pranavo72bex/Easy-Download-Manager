// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A place's right-click menu in the side panel (gaps row U12): a category's Open folder and
//! Add or edit categories…, Unfinished's Resume all and Stop All, Finished's Delete Completed,
//! a queue's Start Queue, Stop Queue and Scheduler…. Its items come from
//! `dm_shell::app::places::place_menu`, drawn as a macOS context menu: rounded and shadowed,
//! the item under the pointer in the selection colour.

use dm_desktop_base::i18n::t;
use dm_shell::app::places::{folder_of, place_menu, Place, PlaceAction};
use gpui::{anchored, deferred, div, prelude::*, Context, Pixels, Point, Role};

use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::menus::Run;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

impl Downloads {
    /// Opens `place`'s menu at `at`, the place chosen first, as a right-click chooses it.
    pub fn open_place_menu(&mut self, place: Place, at: Point<Pixels>) {
        self.choose(place.clone());
        self.menus.place_menu = Some((place, at));
    }

    /// The open place menu's items each given a place for the keys, and the first the keys:
    /// a keyboard user could open it and choose nothing in it (gaps row E50).
    pub(crate) fn place_menu_takes_keys(&mut self, cx: &mut Context<Self>) {
        let Some((place, _)) = self.menus.place_menu.as_ref() else {
            return;
        };
        let count = place_menu(place).len();
        self.menus.place_menu_keys = (0..count).map(|_| cx.focus_handle()).collect();
        self.menus.place_menu_keys_owed = true;
    }

    /// The first item given the keys where they are owed, as the menu is drawn.
    pub(crate) fn place_menu_give_keys(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if !std::mem::take(&mut self.menus.place_menu_keys_owed) {
            return;
        }
        if let Some(first) = self.menus.place_menu_keys.first() {
            window.focus(first, cx);
        }
    }

    /// The keys moved `by` items in the open place menu, round from the last to the first.
    fn place_menu_step(&mut self, by: isize, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let keys = &self.menus.place_menu_keys;
        if keys.is_empty() {
            return;
        }
        let at = keys.iter().position(|k| k.is_focused(window)).unwrap_or(0) as isize;
        let next = (at + by).rem_euclid(keys.len() as isize) as usize;
        let next = keys[next].clone();
        window.focus(&next, cx);
    }

    /// A place's menu as a phone shows it, a sheet from the bottom, the place chosen (a long
    /// press on it in the drawer, gaps row N23).
    pub(crate) fn open_place_sheet(&mut self, place: Place, cx: &mut Context<Self>) {
        self.choose(place.clone());
        self.layout.narrow.drawer = None;
        let items = place_items(&place);
        let menu = gpui::Menu {
            // Headed by the place's name, as a card's sheet by the download's.
            name: crate::window::sidebar::place_name(self)
                .unwrap_or_else(|| t("Place menu").into()),
            items,
            disabled: false,
        };
        self.layout.narrow.sheet = Some(crate::window::narrow::sheet::Sheet::new(menu.owned()));
        cx.notify();
    }

    /// A place sheet's item chosen.
    pub(crate) fn on_place(
        &mut self,
        on: &OnPlace,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.layout.narrow.sheet = None;
        self.act_on_place(on.0.clone(), on.1, window, cx);
    }

    /// Closes the place menu.
    pub fn close_place_menu(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.menus.place_menu = None;
        self.menus.place_menu_keys.clear();
        // The keys back where they were as it opened (gaps row E45).
        if let Some(opener) = self.menus.place_menu_opener.take() {
            window.focus(&opener, cx);
        }
    }

    /// Carries out what the place menu asked for.
    fn act_on_place(
        &mut self,
        place: Place,
        action: PlaceAction,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.close_place_menu(window, cx);
        match action {
            PlaceAction::Run(command) => window.dispatch_action(Box::new(Run(command)), cx),
            PlaceAction::EditCategories => {
                self.open_options_at(dm_desktop_base::options::Tab::SaveTo, cx);
            }
            PlaceAction::OpenFolder => {
                let settings = self.backend.settings();
                let folder = folder_of(
                    &place,
                    &settings.folders.download_dir,
                    &settings.categories(),
                );
                // Made where it is not there yet: a category nothing has gone to has no folder.
                let _ = std::fs::create_dir_all(&folder);
                self.open_path(&folder.to_string_lossy());
            }
        }
        cx.notify();
    }
}

/// A place sheet's items: its place menu's (`places::place_menu`), each run on `place`.
pub(crate) fn place_items(place: &Place) -> Vec<gpui::MenuItem> {
    place_menu(place)
        .into_iter()
        .map(|(words, action)| gpui::MenuItem::action(t(words), OnPlace(place.clone(), action)))
        .collect()
}

/// A place sheet's item: what to do to which place.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = dm, no_json)]
pub struct OnPlace(pub Place, pub PlaceAction);

/// The place menu, open at `at`.
pub fn menu(
    place: &Place,
    at: Point<Pixels>,
    keys: Vec<gpui::FocusHandle>,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let items: Vec<gpui::AnyElement> = place_menu(place)
        .into_iter()
        .enumerate()
        .map(|(n, (words, action))| {
            let place = place.clone();
            let picked = place.clone();
            item(("place-menu", n), t(words), theme)
                .when_some(keys.get(n), |item, key| {
                    item.track_focus(key)
                        .focus(move |s| s.bg(theme.selection).text_color(theme.on_selection))
                        // Its keys stay inside the menu: the arrows and Tab move among its
                        // items, round from the last to the first; Escape closes it (gaps row
                        // E50). Their actions, on each item, as the focused one is first to
                        // hear them: the list's own arrows and Tab came first otherwise.
                        .on_action(cx.listener(
                            |view, _: &crate::window::menus::SelectNext, window, cx| {
                                view.place_menu_step(1, window, cx);
                            },
                        ))
                        .on_action(cx.listener(
                            |view, _: &crate::window::menus::SelectPrevious, window, cx| {
                                view.place_menu_step(-1, window, cx);
                            },
                        ))
                        .on_action(cx.listener(
                            |view, _: &crate::window::menus::FocusNext, window, cx| {
                                view.place_menu_step(1, window, cx);
                            },
                        ))
                        .on_action(cx.listener(
                            |view, _: &crate::window::menus::FocusPrevious, window, cx| {
                                view.place_menu_step(-1, window, cx);
                            },
                        ))
                        // Return picks it, as a click does.
                        .on_action(cx.listener(
                            move |view, _: &crate::window::menus::Confirm, window, cx| {
                                view.act_on_place(picked.clone(), action, window, cx);
                            },
                        ))
                        .on_action(cx.listener(
                            |view, _: &crate::window::menus::Dismiss, window, cx| {
                                view.close_place_menu(window, cx);
                                cx.notify();
                            },
                        ))
                })
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.act_on_place(place.clone(), action, window, cx);
                }))
                .child(screen.label(t(words)))
                .into_any_element()
        })
        .collect();
    deferred(
        anchored().position(at).child(
            div()
                .id("place-menu")
                .role(Role::Menu)
                .aria_label(t("Place menu"))
                .min_w(px(220.0))
                .py_1()
                .rounded(px(space::CORNER_M))
                .border_1()
                .border_color(theme.separator)
                .bg(theme.menu)
                .shadow_lg()
                .text_color(theme.text)
                // The menu takes its own clicks: else a place or row under it would take them too.
                .occlude()
                .on_mouse_down_out(cx.listener(|view, _, window, cx| {
                    view.close_place_menu(window, cx);
                    cx.notify();
                }))
                .children(items),
        ),
    )
    .with_priority(crate::widgets::space::layer::MENU)
}

/// One item of the menu: its words, in the selection colour under the pointer. The caller adds
/// what a click does.
fn item(
    id: impl Into<gpui::ElementId>,
    words: &'static str,
    theme: Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .role(Role::MenuItem)
        .aria_label(words)
        .mx_1()
        .px_2()
        .h(px(22.0))
        .rounded(px(space::CORNER_S))
        .flex()
        .items_center()
        .gap_1()
        .text_size(px(crate::widgets::native::TEXT))
        .hover(|s| s.bg(theme.selection).text_color(theme.on_selection))
}
