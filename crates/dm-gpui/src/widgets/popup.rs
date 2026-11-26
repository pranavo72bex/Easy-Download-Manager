// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A pop-up button, as macOS's own: what is chosen, and a click opens the
//! system's own menu of the choices (an `NSMenu`, through gpui-component's
//! `NativeMenu`; drawn by GPUI where the system has none), the chosen one
//! ticked. The choice comes back as a [`Pick`] action to the window's view.
//! The system's menu waits for a person, so a click the control port gives
//! a pop-up holds its choices instead ([`held`]), and the port's next click
//! on one of their words picks it, as a person's would.

use gpui::{div, prelude::*, Action, App, Role, SharedString, Window};
use gpui_component::native_menu::NativeMenu;

use crate::widgets::space;
use crate::window::a11y::Told;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// The choice `index` made in the pop-up `field` (of a list's `item`,
/// where there is one a row, as a torrent's files each have one).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct Pick {
    /// Which pop-up.
    pub field: &'static str,
    /// Which row's, where each row of a list has one; else 0.
    pub item: usize,
    /// The choice's place in its list.
    pub index: usize,
}

/// A pop-up the control port clicked: its window, which pop-up, and its
/// choices, waiting for the port to click one.
#[derive(Clone)]
pub struct Held {
    pub window: gpui::AnyWindowHandle,
    pub field: &'static str,
    pub item: usize,
    pub choices: Vec<SharedString>,
}

/// A pop-up's choices shown as a bottom sheet on a phone: which pop-up, its choices, the one
/// chosen, and where a finger has dragged the sheet.
#[derive(Clone)]
struct Picking {
    field: &'static str,
    item: usize,
    choices: Vec<SharedString>,
    chosen: Option<usize>,
    drag: crate::widgets::bottomsheet::Drag,
    /// The keys' place on each line, the chosen one (the first where none is) given them as
    /// the sheet opens (gaps row E25), the arrows and Tab moving among them round and never out
    /// of the sheet (gaps row E50).
    keys: Vec<gpui::FocusHandle>,
    /// What had the keys as it opened (the pop-up button): given them back as it closes (gaps
    /// row E46).
    opener: Option<gpui::FocusHandle>,
}

thread_local! {
    /// The pop-up whose choices a phone shows as a sheet, while one does.
    static PICKING: std::cell::RefCell<Option<Picking>> = const { std::cell::RefCell::new(None) };
    /// Whether the click being given is the control port's.
    static SCRIPTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The pop-up the port clicked last, until one of its choices is.
    static HELD: std::cell::RefCell<Option<Held>> = const { std::cell::RefCell::new(None) };
}

/// Gives the control port's click: pop-ups hold their choices instead of
/// opening the system's menu, which would wait for a person.
pub fn scripted<R>(click: impl FnOnce() -> R) -> R {
    SCRIPTED.with(|s| s.set(true));
    let done = click();
    SCRIPTED.with(|s| s.set(false));
    done
}

/// Whether the click being given is the control port's: a menu of the system's it opens is
/// held for the port instead (the row menu's, `probe::hold_menu`).
pub fn scripting() -> bool {
    SCRIPTED.with(|s| s.get())
}

/// The pop-up the port clicked, taken.
pub fn held() -> Option<Held> {
    HELD.with(|h| h.borrow_mut().take())
}

/// The pop-up the port clicked and its window, left held: what a person would see listed.
pub fn held_choices() -> Option<(gpui::AnyWindowHandle, Vec<SharedString>)> {
    HELD.with(|h| {
        h.borrow()
            .as_ref()
            .map(|held| (held.window, held.choices.clone()))
    })
}

/// The pop-up the port clicked, put back (its choice was not among them).
pub fn hold(held: Held) {
    HELD.with(|h| *h.borrow_mut() = Some(held));
}

/// The pop-up `field`, showing `words`; a click opens `choices`, the one at
/// `chosen` ticked.
pub fn button(
    field: &'static str,
    words: SharedString,
    choices: Vec<SharedString>,
    chosen: Option<usize>,
    theme: Theme,
    screen: &Screen,
) -> gpui::Stateful<gpui::Div> {
    button_of(field, 0, words, choices, chosen, theme, screen)
}

/// The pop-up `field` of a list's row `item`, as [`button`].
pub fn button_of(
    field: &'static str,
    item: usize,
    words: SharedString,
    choices: Vec<SharedString>,
    chosen: Option<usize>,
    theme: Theme,
    screen: &Screen,
) -> gpui::Stateful<gpui::Div> {
    use crate::widgets::native::{CONTROL, RADIUS, TEXT};
    // On a phone its choices are a bottom sheet over the window, as a phone's pickers are,
    // drawn by the pop-up itself while open (dm's every sheet: `bottomsheet`).
    let picking = crate::platform::ON_A_PHONE
        .then(|| {
            PICKING.with(|p| {
                p.borrow()
                    .clone()
                    .filter(|open| open.field == field && open.item == item)
            })
        })
        .flatten();
    let expanded = picking.is_some();
    let picker = picking.map(|open| phone_choices(open, theme, screen));
    div()
        .id((field, item))
        .when_some(picker, |button, picker| button.child(picker))
        .role(Role::ComboBox)
        // Says whether its choices are open, as the side panel and Advanced do (gaps row E47).
        .aria_expanded(expanded)
        // Told off to a screen reader where it is greyed (gaps row U23).
        .off_where_greyed()
        // Reached by Tab, opened by Space or Return (gaps row U2).
        .tab_index(0)
        .focus_visible(move |s| s.border_2().border_color(theme.accent_text))
        // Named by what it sets, its choice as its value: named by its choice alone, a screen
        // reader said "00, pop-up button" with no way to tell start from stop or hour from
        // minute.
        .map(|b| match spoken_name(field) {
            Some(name) => b.aria_label(name).aria_value(words.clone()),
            None => b.aria_label(words.clone()),
        })
        .h(px(CONTROL))
        .min_w(px(100.0))
        .pl(px(9.0))
        .pr(px(5.0))
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .gap(px(space::M))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(theme.control_border)
        .bg(theme.control)
        // Answering the pointer and a press as a push button does (gaps row E40).
        .hover(move |s| s.bg(theme.answering(theme.control, crate::window::theme::HOVERED)))
        .active(move |s| s.bg(theme.answering(theme.control, crate::window::theme::PRESSED)))
        .text_size(px(TEXT))
        .whitespace_nowrap()
        .child(screen.label(words))
        // The system's up-and-down chevrons, at the trailing end.
        .child(
            gpui::svg()
                .path("icons/chevron-up-down.svg")
                .size(px(crate::widgets::native::ICON_SMALL))
                .flex_none()
                .text_color(theme.muted),
        )
        .on_click(move |event, window, cx| {
            if SCRIPTED.with(|s| s.get()) {
                hold(Held {
                    window: window.window_handle(),
                    field,
                    item,
                    choices: choices.clone(),
                });
                return;
            }
            if crate::platform::ON_A_PHONE {
                let keys: Vec<gpui::FocusHandle> =
                    choices.iter().map(|_| cx.focus_handle()).collect();
                let opener = window.focused(cx);
                PICKING.with(|p| {
                    *p.borrow_mut() = Some(Picking {
                        field,
                        item,
                        choices: choices.clone(),
                        chosen,
                        drag: Default::default(),
                        keys: keys.clone(),
                        opener,
                    })
                });
                if let Some(first) = keys.get(chosen.unwrap_or(0)).or(keys.first()) {
                    window.focus(first, cx);
                }
                window.refresh();
                return;
            }
            open(field, item, &choices, chosen, event.position(), window, cx)
        })
}

/// What the pop-up `field` sets, said to a screen reader, in the language in force: the label
/// its row shows; `None` where its choice says it alone.
fn spoken_name(field: &str) -> Option<SharedString> {
    use dm_desktop_base::i18n::t;
    let part = |at: &'static str, unit: &'static str| format!("{}, {}", t(at), t(unit)).into();
    Some(match field {
        "start-hour" | "from-hour" | "slow-from-hour" => part("Start at", "Hour"),
        "start-minute" | "from-minute" | "slow-from-minute" => part("Start at", "Minute"),
        "to-hour" | "slow-to-hour" => part("Stop at", "Hour"),
        "to-minute" | "slow-to-minute" => part("Stop at", "Minute"),
        _ => t(match field {
            "date-day" => "Day",
            "date-month" => "Month",
            "date-year" => "Year",
            "priority" => "Priority",
            "category" => "Category",
            "delete-after" => "Delete file after",
            "dialup-connection" => "Connection",
            "digits" => "Digits",
            "duplicate" => "When an address is already in the list",
            "follow-queue" | "links-queue" | "queue-limit" | "queue-start" => "Queue",
            "language" => "Language",
            "network" => "Network",
            "on-close" => "When I close the window",
            "on-metered" => "On a metered connection",
            "proxy-kind" => "Proxy",
            "quality" => "Video quality",
            "recent" => "Recent",
            "same-file" => "When the same file is already here",
            "template" => "Template",
            "theme" => "Theme",
            "third-network" => "Third network",
            "keep-key" => "Hold while clicking to leave it with the browser",
            "take-key" => "Hold while clicking to send it to dm whatever its type or size",
            _ => return None,
        })
        .into(),
    })
}

/// The title of a phone's sheet of `field`'s choices: what it sets, as a screen reader names
/// the pop-up (gaps row E42); "Choose…" only where nothing names it. Every sheet said "Choose…".
fn sheet_title(field: &str) -> SharedString {
    spoken_name(field).unwrap_or_else(|| dm_desktop_base::i18n::t("Choose…").into())
}

/// Closes a pop-up's sheet where one is open, as a phone's Back closes the top layer only:
/// true where one was. Back with a pop-up's choices open closed the whole dialog, and what was
/// typed in it went.
#[cfg(test)]
fn close_picking() -> bool {
    PICKING.with(|p| p.borrow_mut().take().is_some())
}

/// The keys moved `by` lines in a pop-up's open sheet, round from the last to the first.
fn step_picking(by: isize, window: &mut gpui::Window, cx: &mut gpui::App) {
    let keys = PICKING.with(|p| p.borrow().as_ref().map(|open| open.keys.clone()));
    let Some(keys) = keys.filter(|k| !k.is_empty()) else {
        return;
    };
    let at = keys.iter().position(|k| k.is_focused(window)).unwrap_or(0) as isize;
    let next = (at + by).rem_euclid(keys.len() as isize) as usize;
    window.focus(&keys[next], cx);
}

/// Closes a pop-up's sheet (a choice, Close, Back) and gives the keys back to what had them as
/// it opened, the pop-up button (gaps row E46): true where one was open.
pub fn close_picking_in(window: &mut gpui::Window, cx: &mut gpui::App) -> bool {
    let Some(open) = PICKING.with(|p| p.borrow_mut().take()) else {
        return false;
    };
    if let Some(opener) = open.opener {
        window.focus(&opener, cx);
    }
    window.refresh();
    true
}

/// The hooks a pop-up's sheet answers through: its state is [`PICKING`].
fn phone_hooks() -> crate::widgets::bottomsheet::Hooks {
    use std::rc::Rc;
    crate::widgets::bottomsheet::Hooks {
        close: Rc::new(|window, cx| {
            close_picking_in(window, cx);
        }),
        drag: Rc::new(|_| {
            PICKING.with(|p| {
                p.borrow()
                    .as_ref()
                    .map(|open| open.drag)
                    .unwrap_or_default()
            })
        }),
        set_drag: Rc::new(|drag, window, _| {
            PICKING.with(|p| {
                if let Some(open) = p.borrow_mut().as_mut() {
                    open.drag = drag;
                }
            });
            window.refresh();
        }),
    }
}

/// A pop-up's choices as a phone's bottom sheet, over the whole window: a tap on one picks it
/// (sent as a [`Pick`] to the window, as the system's menu sends it) and closes the sheet.
fn phone_choices(open: Picking, theme: Theme, screen: &Screen) -> impl IntoElement {
    use crate::widgets::bottomsheet;
    let hooks = phone_hooks();
    let title = sheet_title(open.field);
    let items: Vec<gpui::AnyElement> = open
        .choices
        .iter()
        .enumerate()
        .map(|(index, words)| {
            let field = open.field;
            let item = open.item;
            let ticked = open.chosen == Some(index);
            bottomsheet::line(("pick", index), theme)
                .when_some(open.keys.get(index).cloned(), |l, key| {
                    use crate::window::menus::{
                        FocusNext, FocusPrevious, SelectNext, SelectPrevious,
                    };
                    l.track_focus(&key)
                        .on_action(|_: &SelectNext, window, cx| step_picking(1, window, cx))
                        .on_action(|_: &SelectPrevious, window, cx| step_picking(-1, window, cx))
                        .on_action(|_: &FocusNext, window, cx| step_picking(1, window, cx))
                        .on_action(|_: &FocusPrevious, window, cx| step_picking(-1, window, cx))
                })
                .role(Role::MenuItem)
                .aria_label(words.clone())
                // The tick told, as it is drawn (gaps row E25).
                .when(ticked, |l| l.aria_toggled(gpui::Toggled::True))
                .on_click(move |_, window, cx| {
                    close_picking_in(window, cx);
                    window.dispatch_action(Box::new(Pick { field, item, index }), cx);
                    window.refresh();
                })
                .child(
                    div()
                        .flex_none()
                        .w(px(20.0))
                        .flex()
                        .items_center()
                        .when(ticked, |d| {
                            d.child(crate::widgets::native::icon(
                                "icons/check.svg",
                                crate::widgets::native::ICON,
                                theme.accent_text,
                            ))
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .when(ticked, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                        .child(screen.label(words.clone())),
                )
                .into_any_element()
        })
        .collect();
    let screen = screen.clone();
    let drag = open.drag;
    // Drawn over the whole window, whatever the pop-up's own place: deferred, at the window's
    // origin, a layer the window's size.
    gpui::deferred(
        gpui::anchored()
            .position(gpui::point(gpui::px(0.0), gpui::px(0.0)))
            .child(PickerLayer {
                title,
                items: std::cell::RefCell::new(Some(items)),
                drag,
                hooks,
                theme,
                screen,
            }),
    )
    .with_priority(crate::widgets::space::layer::SHEET)
}

/// The layer a pop-up's sheet stands on: the window's size, dimmed, the sheet at its foot.
#[derive(IntoElement)]
struct PickerLayer {
    title: SharedString,
    items: std::cell::RefCell<Option<Vec<gpui::AnyElement>>>,
    drag: crate::widgets::bottomsheet::Drag,
    hooks: crate::widgets::bottomsheet::Hooks,
    theme: Theme,
    screen: Screen,
}

impl RenderOnce for PickerLayer {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        use crate::widgets::bottomsheet;
        let viewport = window.viewport_size();
        let items = self.items.borrow_mut().take().unwrap_or_default();
        let name = div()
            .id("picker-name")
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(self.screen.label(self.title.clone()));
        div()
            .w(viewport.width)
            .h(viewport.height)
            .child(bottomsheet::scrim("picker-scrim", &self.hooks))
            .child(bottomsheet::sheet(
                "picker",
                self.title.clone(),
                name,
                false,
                items,
                self.drag,
                &self.hooks,
                self.theme,
                &self.screen,
                window,
            ))
    }
}

/// The system's menu of `choices`, at `at`.
fn open(
    field: &'static str,
    item: usize,
    choices: &[SharedString],
    chosen: Option<usize>,
    at: gpui::Point<gpui::Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let menu = choices
        .iter()
        .enumerate()
        .fold(NativeMenu::new(), |menu, (index, words)| {
            menu.menu_with_check(
                words.clone(),
                chosen == Some(index),
                Box::new(Pick { field, item, index }),
            )
        });
    menu.show(at, window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pop-up is named by what it sets, an hour's and a minute's by their row too; one whose
    /// choice says it alone has no name of its own (gaps row E27).
    #[test]
    fn a_popup_is_named_by_what_it_sets() {
        assert_eq!(spoken_name("from-hour").as_deref(), Some("Start at, Hour"));
        assert_eq!(spoken_name("to-minute").as_deref(), Some("Stop at, Minute"));
        assert_eq!(spoken_name("priority").as_deref(), Some("Priority"));
        assert_eq!(spoken_name("no-such-field"), None);
    }

    /// A phone's sheet of a pop-up's choices is titled by what it sets, as a screen reader
    /// names the pop-up (gaps row E42); "Choose…" only where nothing names it.
    #[test]
    fn a_popups_sheet_is_titled_by_what_it_sets() {
        assert_eq!(sheet_title("priority").as_ref(), "Priority");
        assert_eq!(sheet_title("from-hour").as_ref(), "Start at, Hour");
        assert_eq!(sheet_title("no-such-field").as_ref(), "Choose…");
    }

    /// Back with a pop-up's sheet open closes the sheet alone, once: the next Back is the
    /// dialog's (gaps row E27).
    #[test]
    fn back_closes_a_popups_sheet_before_its_dialog() {
        assert!(!close_picking(), "nothing was open");
        PICKING.with(|p| {
            *p.borrow_mut() = Some(Picking {
                field: "priority",
                item: 0,
                choices: vec!["Low".into(), "High".into()],
                chosen: Some(0),
                drag: Default::default(),
                keys: Vec::new(),
                opener: None,
            })
        });
        assert!(close_picking(), "the sheet was open");
        assert!(!close_picking(), "the next Back is the dialog's");
    }
}
