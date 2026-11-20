// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A menu drawn in the window at a phone's width, as a sheet under the top bar: the overflow
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

/// An item's height: a finger's, as a phone's menus have them.
const ITEM_HEIGHT: f32 = 40.0;

/// The sheet's widest, in points.
const WIDEST: f32 = 320.0;

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
    let width = (f32::from(viewport.width) - crate::window::zoom::of(2.0 * space::XL))
        .min(crate::window::zoom::of(WIDEST));
    let top = crate::window::titlebar::height();
    let walked = sheet.pages.len() > 1;
    let mut panel = div()
        .id("sheet")
        .role(Role::Menu)
        .absolute()
        .top(gpui::px(top))
        .right(px(space::XS))
        .w(gpui::px(width.max(0.0)))
        .max_h(viewport.height - gpui::px(top) - px(space::XL))
        .overflow_y_scroll()
        .py(px(space::XS))
        .flex()
        .flex_col()
        .rounded(px(space::CORNER_L))
        .border_1()
        .border_color(theme.separator)
        .bg(theme.menu)
        .text_color(theme.text)
        .occlude();
    let Some(menu) = sheet.shown() else {
        return panel;
    };
    panel = panel.aria_label(menu.name.clone());
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
                    .flex_1()
                    .min_w_0()
                    .child(screen.label(sub.name.clone())),
            )
            .child(div().flex_none().text_color(theme.muted).child("›"))
            .into_any_element(),
        OwnedMenuItem::Action {
            name,
            checked,
            disabled,
            ..
        } => {
            let disabled = *disabled;
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
                        .w(px(space::L))
                        .when(*checked, |d| d.child("✓")),
                )
                .child(div().flex_1().min_w_0().child(screen.label(name.clone())))
                .into_any_element()
        }
        OwnedMenuItem::SystemMenu(_) => div().into_any_element(),
    }
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
