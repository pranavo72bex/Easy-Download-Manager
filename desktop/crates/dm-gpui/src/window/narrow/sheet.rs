// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A menu drawn in the window at a phone's width, as a sheet up from the screen's foot: the overflow
//! menu (the toolbar's commands, then Tasks, File, Downloads, View and Help) and a card's row
//! menu. Its items are the desktop's menus' own (`gpui::OwnedMenu`), so each runs its action
//! as the menu bar's item does; a submenu opens in the sheet's place, with its name at the top
//! to go back. Drawn by dm, not the system's, as a phone has no menus of the system's to show.

use dm_desktop_base::i18n::t;
use gpui::{div, prelude::*, Context, Div, OwnedMenu, OwnedMenuItem, Role, Stateful, Window};

use crate::widgets::{native, space};
use crate::window::list::Downloads;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// An item's height: a finger's target, as a phone's menus have them.
const ITEM_HEIGHT: f32 = space::TOUCH;

/// A toolbar command's picture before its words, in points.
const ICON: f32 = 20.0;

/// A sheet open: the menu it opened with, and the submenus walked into, the last shown.
#[derive(Clone)]
pub(crate) struct Sheet {
    pages: Vec<OwnedMenu>,
}

impl Sheet {
    /// A sheet showing `menu`.
    pub(crate) fn new(menu: OwnedMenu) -> Sheet {
        Sheet { pages: vec![menu] }
    }

    /// The menu shown.
    fn shown(&self) -> Option<&OwnedMenu> {
        self.pages.last()
    }

    /// Back to the menu the submenu shown opened from; false on the sheet's first menu.
    pub(crate) fn back(&mut self) -> bool {
        if self.pages.len() < 2 {
            return false;
        }
        self.pages.pop();
        true
    }
}

/// The overflow menu: the toolbar's buttons as the person keeps them (their words as the
/// toolbar says them, greyed where the toolbar's are), then the window's menus, each a
/// submenu, with the menu bar's items and ticks.
pub(super) fn overflow(view: &Downloads) -> OwnedMenu {
    use crate::window::menus::Run;
    use dm_shell::app::commands::Command;
    let resume = dm_desktop_base::rows::resume_word(&view.chosen());
    let mut items: Vec<gpui::MenuItem> = dm_shell::app::commands::toolbar_shown(&view.known.drawn)
        .into_iter()
        .map(|(command, words)| {
            let words =
                if command == Command::DownloadNow && matches!(resume, "Retry" | "Start now") {
                    t(resume)
                } else {
                    t(words)
                };
            gpui::MenuItem::action(words, Run(command)).disabled(!view.enabled(command))
        })
        .collect();
    let ticks = view.menus.menu_ticks.clone().unwrap_or_default();
    items.push(gpui::MenuItem::separator());
    items.extend(
        crate::window::menus::window_menus(&ticks)
            .into_iter()
            .map(gpui::MenuItem::submenu),
    );
    gpui::Menu::new(t("Menu")).items(items).owned()
}

/// The sheet, under the top bar at the window's right.
pub(super) fn drawn(
    sheet: &Sheet,
    theme: Theme,
    screen: &Screen,
    window: &mut Window,
    cx: &mut Context<Downloads>,
) -> Stateful<Div> {
    let viewport = window.viewport_size();
    // Under the top bar, which sits below a phone's status bar; clear of its navigation bar
    // (gaps row N16).
    let safe = crate::window::safe::edges(window);
    let top = crate::window::titlebar::narrow_height() + f32::from(safe.top);
    let walked = sheet.pages.len() > 1;
    // A bottom sheet, as a phone's menus are: across the screen's foot where the thumb is,
    // over the navigation bar's room, at most up to the top bar.
    let mut panel = div()
        .id("sheet")
        .role(Role::Menu)
        .absolute()
        .bottom_0()
        .left_0()
        .right_0()
        .max_h(viewport.height - gpui::px(top))
        .overflow_y_scroll()
        .pt(px(space::M))
        .pb(px(space::M) + safe.bottom)
        .pl(safe.left)
        .pr(safe.right)
        .flex()
        .flex_col()
        .rounded_t(px(space::CORNER_L * 2.0))
        .border_t_1()
        .border_color(theme.separator)
        .bg(theme.menu)
        .text_color(theme.text)
        .occlude();
    let Some(menu) = sheet.shown() else {
        return panel;
    };
    panel = panel.aria_label(menu.name.clone());
    // A bottom sheet's handle, as a phone's sheets have one.
    panel = panel.child(
        div()
            .flex_none()
            .w_full()
            .flex()
            .justify_center()
            .pb(px(space::S))
            .child(div().w(px(36.0)).h(px(4.0)).rounded_full().bg(theme.muted)),
    );
    // The sheet's name at its top: a card's file, or dm's menu.
    if !walked {
        panel = panel.child(
            div()
                .flex_none()
                .px(px(space::L))
                .pb(px(space::S))
                .mb(px(space::XS))
                .border_b_1()
                .border_color(theme.separator)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(native::TEXT + 2.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(screen.label(menu.name.clone())),
        );
    }
    // A submenu's name at the top: a tap goes back to the menu it opened from.
    if walked {
        panel = panel.child(
            line(("sheet-back", 0usize), theme)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .on_click(cx.listener(|view, _, _, cx| {
                    if let Some(sheet) = view.layout.narrow.sheet.as_mut() {
                        sheet.pages.pop();
                    }
                    cx.notify();
                }))
                .child(div().flex_none().child("‹"))
                .child(screen.label(menu.name.clone())),
        );
    }
    for (i, item) in menu.items.iter().enumerate() {
        panel = panel.child(item_drawn(i, item, theme, screen, cx));
    }
    panel
}

/// One line of the sheet, a finger's height.
fn line(id: impl Into<gpui::ElementId>, theme: Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(ITEM_HEIGHT))
        .px(px(space::L))
        .flex()
        .items_center()
        .gap(px(space::M))
        .text_size(px(native::TEXT))
        .hover(|l| l.bg(theme.control))
}

/// Item `i` of the menu shown.
fn item_drawn(
    i: usize,
    item: &OwnedMenuItem,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> gpui::AnyElement {
    match item {
        OwnedMenuItem::Separator => div()
            .flex_none()
            .my(px(space::XS))
            .h(px(space::LINE))
            .bg(theme.separator)
            .into_any_element(),
        OwnedMenuItem::Submenu(sub) => line(("sheet-item", i), theme)
            .role(Role::MenuItem)
            .aria_label(sub.name.clone())
            .on_click(cx.listener(move |view, _, _, cx| {
                view.sheet_submenu(i);
                cx.notify();
            }))
            .child(
                div()
                    .flex_none()
                    .w(px(ICON))
                    .flex()
                    .justify_center()
                    .when_some(menu_picture(&sub.name), |d, picture| {
                        d.child(
                            gpui::svg()
                                .path(picture)
                                .size(px(ICON))
                                .text_color(theme.muted),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(screen.label(sub.name.clone())),
            )
            .child(div().flex_none().text_color(theme.muted).child("›"))
            .into_any_element(),
        OwnedMenuItem::Action {
            name,
            action,
            checked,
            disabled,
            ..
        } => {
            let disabled = *disabled;
            // A command's picture, as its toolbar button has it on a desktop.
            let picture = picture_of(action.as_ref());
            line(("sheet-item", i), theme)
                .role(Role::MenuItem)
                .aria_label(gpui::SharedString::from(name.clone()))
                .when(disabled, |l| l.text_color(theme.muted))
                .when(!disabled, |l| {
                    l.on_click(cx.listener(move |view, _, window, cx| {
                        view.sheet_action(i, window, cx);
                    }))
                })
                .child(
                    div()
                        .flex_none()
                        .w(px(ICON))
                        .flex()
                        .justify_center()
                        .when_some(picture, |d, picture| {
                            d.child(
                                gpui::svg()
                                    .path(picture)
                                    .size(px(ICON))
                                    .text_color(if disabled { theme.muted } else { theme.text }),
                            )
                        })
                        .when(*checked && picture.is_none(), |d| d.child("✓")),
                )
                .child(div().flex_1().min_w_0().child(screen.label(name.clone())))
                .into_any_element()
        }
        OwnedMenuItem::SystemMenu(_) => div().into_any_element(),
    }
}

/// The picture beside a sheet's item that runs `action`: a toolbar command's own, and the row
/// menu's commands' where one says them; none for the rest.
fn picture_of(action: &dyn gpui::Action) -> Option<&'static str> {
    use dm_shell::app::commands::Command as C;
    if action.as_any().is::<crate::window::rowmenu::ShareFile>() {
        return Some("icons/share.svg");
    }
    let command = action
        .as_any()
        .downcast_ref::<crate::window::menus::Run>()?
        .0;
    if dm_shell::app::commands::TOOLBAR
        .iter()
        .any(|(on_bar, _)| *on_bar == command)
    {
        return Some(crate::window::toolbar::icon(command));
    }
    Some(match command {
        C::OpenFile | C::OpenWith | C::Preview => "icons/open.svg",
        C::OpenFolder => "icons/place-folder.svg",
        C::DeleteFile => "icons/delete.svg",
        C::Properties => "icons/details.svg",
        C::CopyLink | C::UpdateLink => "icons/link.svg",
        C::Redownload => "icons/resume.svg",
        C::MoveToQueue | C::RemoveFromQueue => "icons/place-queue.svg",
        _ => return None,
    })
}

/// The picture beside one of the window's menus in the overflow sheet, by its name.
fn menu_picture(name: &str) -> Option<&'static str> {
    use dm_desktop_base::i18n::t;
    let pictures = [
        (t("Tasks"), "icons/start-queue.svg"),
        (t("File"), "icons/place-folder.svg"),
        (t("Downloads"), "icons/place-all.svg"),
        (t("View"), "icons/sidebar.svg"),
        (t("Help"), "icons/details.svg"),
    ];
    pictures
        .into_iter()
        .find(|(words, _)| *words == name)
        .map(|(_, picture)| picture)
}

impl Downloads {
    /// Submenu `i` of the menu shown opened in the sheet's place.
    fn sheet_submenu(&mut self, i: usize) {
        let Some(sheet) = self.layout.narrow.sheet.as_mut() else {
            return;
        };
        let sub = match sheet.shown().and_then(|menu| menu.items.get(i)) {
            Some(OwnedMenuItem::Submenu(sub)) => sub.clone(),
            _ => return,
        };
        sheet.pages.push(sub);
    }

    /// Item `i` of the menu shown, picked: the sheet closes and its action runs in the window,
    /// as the menu bar's item sends it.
    fn sheet_action(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let action = match self
            .layout
            .narrow
            .sheet
            .take()
            .as_ref()
            .and_then(Sheet::shown)
            .and_then(|menu| menu.items.get(i))
        {
            Some(OwnedMenuItem::Action { action, .. }) => action.boxed_clone(),
            _ => return,
        };
        let focus = self.focus.clone();
        // After this listener, with the window's view free to take it.
        window.defer(cx, move |window, cx| {
            window.focus(&focus, cx);
            focus.dispatch_action(action.as_ref(), window, cx);
        });
        cx.notify();
    }
}
