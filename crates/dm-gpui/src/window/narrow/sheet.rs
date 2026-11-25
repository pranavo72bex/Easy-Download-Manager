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

use crate::widgets::{bottomsheet, space};
use crate::window::a11y::Told;
use crate::window::list::Downloads;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// A toolbar command's picture before its words, in points.
const ICON: f32 = crate::widgets::native::ICON_LARGE;

/// A sheet open: the menu it opened with, and the submenus walked into, the last shown.
#[derive(Clone)]
pub(crate) struct Sheet {
    pages: Vec<OwnedMenu>,
    /// Where a finger has dragged it.
    drag: bottomsheet::Drag,
    /// Items of the first menu drawn as big buttons in a row above the others, and left out of
    /// the list (a card's Open, Share, Delete and Pause or Resume, gaps row C22).
    quick: Vec<usize>,
    /// The keys' place on its first line, moved there as it opens and as a submenu is walked
    /// into (gaps row E25): the keys stayed on the window behind it.
    first: Option<gpui::FocusHandle>,
    /// How many pages were shown when the first line was last given the keys.
    focused_for: usize,
}

impl Sheet {
    /// A sheet showing `menu`.
    pub(crate) fn new(menu: OwnedMenu) -> Sheet {
        Sheet {
            pages: vec![menu],
            drag: bottomsheet::Drag::default(),
            quick: Vec::new(),
            first: None,
            focused_for: 0,
        }
    }

    /// A sheet showing `menu`, the items picked by `quick` drawn as big buttons above the rest.
    pub(crate) fn with_quick(menu: OwnedMenu, quick: impl Fn(&OwnedMenuItem) -> bool) -> Sheet {
        let picked = menu
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                matches!(
                    item,
                    OwnedMenuItem::Action {
                        disabled: false,
                        ..
                    }
                ) && quick(item)
            })
            .map(|(i, _)| i)
            .collect();
        Sheet {
            quick: picked,
            ..Sheet::new(menu)
        }
    }

    /// The keys given to its first line where the page shown is new to them.
    pub(crate) fn focus_first(&mut self, window: &mut Window, cx: &mut gpui::App) {
        let first = self.first.get_or_insert_with(|| cx.focus_handle()).clone();
        if self.focused_for != self.pages.len() {
            self.focused_for = self.pages.len();
            window.focus(&first, cx);
        }
    }

    /// Whether its first line has the keys.
    #[cfg(test)]
    pub(crate) fn has_keys(&self, window: &Window) -> bool {
        self.first
            .as_ref()
            .is_some_and(|first| first.is_focused(window))
    }

    /// How many of its items are big buttons above the list.
    #[cfg(test)]
    pub(crate) fn quick_count(&self) -> usize {
        self.quick.len()
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
    overflow_on(view, crate::platform::ON_A_PHONE)
}

/// [`overflow`] as a phone (`phone`) or a computer's narrowed window draws it.
pub(crate) fn overflow_on(view: &Downloads, phone: bool) -> OwnedMenu {
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
        crate::window::menus::window_menus_on(&ticks, phone)
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
    let walked = sheet.pages.len() > 1;
    let Some(menu) = sheet.shown() else {
        return div().id("sheet");
    };
    // Its name at the top: a card's file, dm's menu, or the submenu walked into, whose tap
    // goes back to the menu it opened from.
    let name = if walked {
        div()
            .id("sheet-back")
            .h(px(bottomsheet::ITEM_HEIGHT))
            .flex()
            .items_center()
            .gap(px(space::M))
            .on_click(cx.listener(|view, _, _, cx| {
                if let Some(sheet) = view.layout.narrow.sheet.as_mut() {
                    sheet.pages.pop();
                }
                cx.notify();
            }))
            .child(crate::widgets::native::icon(
                crate::window::mirror::back(),
                crate::widgets::native::ICON,
                theme.text,
            ))
            .child(crate::widgets::native::cut(
                "sheet-back-name",
                menu.name.clone(),
                false,
                screen,
            ))
    } else {
        crate::widgets::native::cut("sheet-name", menu.name.clone(), false, screen)
    };
    let quick: &[usize] = if walked { &[] } else { &sheet.quick };
    // The line the keys are given as it opens: the first that can be picked.
    let first_line = menu.items.iter().enumerate().position(|(i, item)| {
        !quick.contains(&i)
            && matches!(
                item,
                OwnedMenuItem::Submenu(_)
                    | OwnedMenuItem::Action {
                        disabled: false,
                        ..
                    }
            )
    });
    let mut items: Vec<gpui::AnyElement> = Vec::new();
    if !quick.is_empty() {
        items.push(quick_row(menu, quick, theme, screen, cx));
    }
    items.extend(
        menu.items
            .iter()
            .enumerate()
            // What the row above holds is not listed again; nor a separator it leaves first.
            .filter(|(i, _)| !quick.contains(i))
            .map(|(i, item)| {
                let first = (Some(i) == first_line)
                    .then_some(sheet.first.as_ref())
                    .flatten();
                item_drawn(i, item, first, theme, screen, cx)
            }),
    );
    bottomsheet::sheet(
        "sheet",
        menu.name.clone(),
        name,
        walked,
        items,
        sheet.drag,
        &hooks(cx),
        theme,
        screen,
        window,
    )
}

/// The big buttons above a card's menu: each item of `quick` as its picture over its words,
/// side by side across the sheet, picked as its line in the list would be.
fn quick_row(
    menu: &OwnedMenu,
    quick: &[usize],
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> gpui::AnyElement {
    div()
        .id("sheet-quick")
        .w_full()
        .flex()
        .gap(px(space::S))
        .px(px(space::L))
        .pb(px(space::S))
        .children(quick.iter().filter_map(|&i| {
            let OwnedMenuItem::Action { name, action, .. } = menu.items.get(i)? else {
                return None;
            };
            let words = name.split('\t').next().unwrap_or(name).to_string();
            let picture = picture_of(action.as_ref());
            Some(
                div()
                    .id(("sheet-quick", i))
                    .role(Role::Button)
                    .aria_label(gpui::SharedString::from(words.clone()))
                    .flex_1()
                    .min_w_0()
                    .py(px(space::M))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(space::XS))
                    .map(|c| crate::widgets::native::faced(c, theme.stripe, false, theme))
                    .rounded(px(crate::widgets::native::RADIUS))
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.sheet_action(i, window, cx);
                    }))
                    .when_some(picture, |d, picture| {
                        d.child(
                            gpui::svg()
                                .path(picture)
                                .size(px(ICON + 4.0))
                                .text_color(theme.text),
                        )
                    })
                    .child(
                        div()
                            .max_w_full()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(screen.label(gpui::SharedString::from(words))),
                    ),
            )
        }))
        .into_any_element()
}

/// What the shared sheet asks of the list's view: the sheet it keeps.
pub(super) fn hooks(cx: &mut Context<Downloads>) -> bottomsheet::Hooks {
    let view = cx.entity().downgrade();
    let (close_view, read_view, set_view) = (view.clone(), view.clone(), view);
    bottomsheet::Hooks {
        close: std::rc::Rc::new(move |_, cx| {
            let _ = close_view.update(cx, |view, cx| {
                view.layout.narrow.sheet = None;
                cx.notify();
            });
        }),
        drag: std::rc::Rc::new(move |cx| {
            read_view
                .upgrade()
                .and_then(|view| view.read(cx).layout.narrow.sheet.as_ref().map(|s| s.drag))
                .unwrap_or_default()
        }),
        set_drag: std::rc::Rc::new(move |drag, _, cx| {
            let _ = set_view.update(cx, |view, cx| {
                if let Some(sheet) = view.layout.narrow.sheet.as_mut() {
                    sheet.drag = drag;
                }
                cx.notify();
            });
        }),
    }
}

/// Item `i` of the menu shown.
fn item_drawn(
    i: usize,
    item: &OwnedMenuItem,
    first: Option<&gpui::FocusHandle>,
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
        OwnedMenuItem::Submenu(sub) => bottomsheet::line(("sheet-item", i), theme)
            .when_some(first, |l, first| l.track_focus(first))
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
            .child(crate::widgets::native::icon(
                crate::window::mirror::forward(),
                crate::widgets::native::ICON_SMALL,
                theme.muted,
            ))
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
            bottomsheet::line(("sheet-item", i), theme)
                .when_some(first, |l, first| l.track_focus(first))
                .role(Role::MenuItem)
                .aria_label(gpui::SharedString::from(name.clone()))
                // Told off and ticked, as it is drawn (gaps row E25).
                .off_if(disabled)
                .when(*checked, |l| l.aria_toggled(gpui::Toggled::True))
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
                        .when(*checked && picture.is_none(), |d| {
                            d.child(crate::widgets::native::icon(
                                "icons/check.svg",
                                ICON,
                                theme.accent_text,
                            ))
                        }),
                )
                .child(div().flex_1().min_w_0().child(screen.label(name.clone())))
                // Where its picture has the tick's place, the tick at the line's end.
                .when(*checked && picture.is_some(), |l| {
                    l.child(crate::widgets::native::icon(
                        "icons/check.svg",
                        ICON,
                        theme.accent_text,
                    ))
                })
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
    if action
        .as_any()
        .is::<crate::window::rowmenu::ChooseSeveral>()
    {
        return Some("icons/place-finished.svg");
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
