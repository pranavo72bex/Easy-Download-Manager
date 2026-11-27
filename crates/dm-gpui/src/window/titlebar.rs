// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's top bar, as a code editor's title bar holds its menus and
//! command centre (the maintainer's words: "take reference
//! from vscode", "make the search bar and options like Tasks File Downloads
//! View Help to top bar", on macOS, Windows and Linux): the system's traffic
//! lights at the left on macOS; the button that shows or hides the
//! categories (⌃⌘S as well); the menus' titles, Tasks, File, Downloads,
//! View and Help, each opening its menu as the system draws one
//! (`menus::window_menus`); and, centred in the room left, the search box
//! that narrows the list as one types (its name, address or note), how
//! many rows it leaves and a way to clear
//! it. On Windows the bar ends with the window's own buttons. The bar's
//! empty space moves the window; a double-click on it does what the
//! system's settings say (zoom or minimize).

use dm_desktop_base::i18n::{t, tn};
use gpui::{
    div, point, prelude::*, svg, Context, MouseButton, Role, SharedString, Window,
    WindowControlArea,
};
use gpui_component::input::Input;
use gpui_component::native_menu::NativeMenu;

use crate::widgets::native;
use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

mod search;
use search::{search_box, search_focused, search_width, shut_look};

/// Whether the top bar is the window's title bar (macOS, Windows), or a
/// row under the desktop's own (Linux, where a program drawing its title
/// bar would draw its own frame and edges too).
pub const DRAWN_BY_DM: bool = cfg!(any(target_os = "macos", windows));

/// The top bar's height at Medium: the traffic lights centred in it.
pub const HEIGHT: f32 = 38.0;

/// Where the system draws the traffic lights, in a bar of `HEIGHT`.
const TRAFFIC_LIGHTS_AT: (f32, f32) = (14.0, 12.0);

/// The bar's height at the text size in force: taller with a larger size,
/// as what it holds grows, but never shorter than the system's own traffic
/// lights and window buttons need, which keep their size.
pub fn height() -> f32 {
    HEIGHT.max(crate::window::zoom::of(HEIGHT))
}

/// The bar's height at a phone's width (gaps row N7): tall enough for a finger's target
/// ([`space::TOUCH`]), else as [`height`].
pub fn narrow_height() -> f32 {
    // A phone's: room for the title and the line under it that says how the downloads go.
    let least = if crate::platform::ON_A_PHONE {
        PHONE_BAR
    } else {
        space::TOUCH
    };
    height().max(crate::window::zoom::of(least))
}

/// A phone's top bar's height: Material's top app bar (64 dp).
const PHONE_BAR: f32 = 64.0;

/// Where the traffic lights go, centred in the bar as tall as it is when
/// the window opens: the system places them once, so a size chosen later
/// centres them at the next start.
pub fn traffic_lights_at() -> (f32, f32) {
    (
        TRAFFIC_LIGHTS_AT.0,
        TRAFFIC_LIGHTS_AT.1 + (height() - HEIGHT) / 2.0,
    )
}

/// Where the bar's own controls begin: past the traffic lights on macOS,
/// at the window's edge elsewhere. In points on screen, as the lights'
/// place is, whatever the text size.
const CONTROLS_AT: f32 = native::Each {
    macos: 78.0,
    windows: 8.0,
    gnome: 8.0,
}
.here();

/// Whether the menus' titles are drawn in the window: not on macOS, where the system's menu bar
/// holds them, but in the window tests, which open menus by them on every system.
const MENU_TITLES_IN_WINDOW: bool = !cfg!(target_os = "macos") || cfg!(test);

/// The top bar.
pub fn titlebar(
    view: &Downloads,
    theme: Theme,
    screen: &Screen,
    window: &mut Window,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    // Finder's search (the maintainer, 2026-11-27: "make similar of Finder application
    // (animation + compact)"): a magnifier at the bar's trailing end, opened to a field while it
    // has the keys or holds words, shut again once it is left empty; its width glides between.
    let focused = search_focused(view, window, cx);
    let open = focused || !view.list.filter.read(cx).value().trim().is_empty();
    let width = search_width(open, window, cx);
    div()
        .id("titlebar")
        .role(Role::Group)
        .aria_label(t("Filter"))
        .h(gpui::px(height()))
        .flex_none()
        .flex()
        .items_center()
        .bg(crate::window::theme::frame(theme.sidebar))
        .border_b_1()
        .border_color(theme.separator)
        .child(drag_space().w(gpui::px(CONTROLS_AT)).flex_none())
        .child(
            // Between the system's own buttons, the bar's: the side panel's switch, the title or
            // the menus' titles, and the search, read from the right in Arabic and Hebrew (they
            // stood from the left, gaps row U35), the traffic lights and Windows' own buttons
            // where the system puts them.
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .map(crate::window::mirror::row)
                .items_center()
                .child(sidebar_button(view.layout.show_sidebar, theme, cx))
                // On macOS, where the menus' titles are the system's bar's, the window's title is the
                // place shown, as Finder titles its window with the folder shown (the maintainer's
                // choice of 2026-11-26, "Page title", read as Finder reads it): "All downloads", a kind,
                // a queue. Windows and Linux draw the menus' titles there.
                .when(!MENU_TITLES_IN_WINDOW, |bar| {
                    let name = crate::window::sidebar::place_name(view)
                        .unwrap_or_else(|| t("All downloads").into());
                    bar.child(
                        div()
                            // Room from the switch on either side, as the bar mirrors.
                            .px(px(space::S))
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(screen.label(name)),
                    )
                })
                // The menus' titles in the window where the system has no menu bar of its own (Windows,
                // Linux); on macOS they are the system's menu bar's, not drawn again here (the
                // maintainer, 2026-11-27: "it is not hiding the Task File Download View Help on Macos. It
                // should show on windows and linux"). The window tests open menus by these titles, as a
                // Windows or Linux person does, on every system: the macOS bar is built from the same
                // menus (`menus::set`).
                .when(MENU_TITLES_IN_WINDOW, |bar| {
                    bar.child(
                        div()
                            .id("menu-titles")
                            .role(Role::MenuBar)
                            .aria_label(t("Menu"))
                            .relative()
                            .flex()
                            .flex_none()
                            .items_center()
                            .ml(px(space::XS))
                            .children(
                                crate::window::menus::WINDOW_MENUS.iter().enumerate().map(
                                    |(i, name)| menu_title(i, t(name).into(), theme, screen, cx),
                                ),
                            ),
                    )
                })
                // The empty space before the search box moves the window; the controls take their own
                // clicks.
                .child(drag_space().flex_1())
                .child(
                    search_box(view, Some(width), !open, focused, theme, screen, cx)
                        .when(!open, |b| shut_look(b, theme, cx)),
                )
                .child(drag_space().w(px(space::M)).flex_none()),
        )
        .children(window_buttons(theme, window.is_maximized()))
}

/// A button of a phone's top bar: a finger's target ([`space::TOUCH`]), its picture
/// [`phone_icon`]; its own click, not the bar's, whose double-click zooms the window.
fn phone_button(id: &'static str, words: &'static str, theme: Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(words)
        .flex_none()
        .min_w(px(space::TOUCH))
        .h(px(space::TOUCH))
        .px(px(space::XS))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(native::RADIUS))
        .hover(|b| b.bg(theme.control))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// A phone's top bar's picture, `path` in `colour`, at the bar's size.
fn phone_icon(path: &'static str, colour: gpui::Rgba) -> gpui::Svg {
    svg().path(path).size(px(PHONE_ICON)).text_color(colour)
}

/// A phone's top bar's pictures' size, in points.
const PHONE_ICON: f32 = 24.0;

/// The top bar at a phone's width (gaps row N7), in place of the menus' titles, the search
/// box and the toolbar: the place chosen's name ("All downloads" at All) as its title, search, and the
/// overflow menu holding the toolbar's commands and the menus (`narrow.rs`). Search
/// opens the desktop's own search box across the bar, the same words narrowing the same list,
/// with a close that clears them. Still the window's title bar where dm draws one: past the
/// traffic lights, its empty space moving the window, the window's own buttons at its end on
/// Windows. Each button a finger's target ([`space::TOUCH`]).
pub fn narrow_titlebar(
    view: &Downloads,
    theme: Theme,
    screen: &Screen,
    window: &mut Window,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let button = |id: &'static str, words: &'static str| phone_button(id, words, theme);
    let icon = phone_icon;
    let menu = t("Menu");
    let filter = t("Filter");
    let close = t("Close");
    let bar = div()
        .id("titlebar")
        .role(Role::Group)
        .aria_label(menu)
        .h(gpui::px(narrow_height()))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(space::XS))
        .bg(crate::window::theme::frame(theme.sidebar))
        .border_b_1()
        .border_color(theme.separator)
        .child(drag_space().w(gpui::px(CONTROLS_AT)).flex_none());
    let bar = if view.layout.narrow.choosing {
        choosing_bar(bar, view, theme, screen, cx)
    } else if view.narrow_searching() {
        bar.child(search_box(
            view,
            None,
            false,
            search_focused(view, window, cx),
            theme,
            screen,
            cx,
        ))
        .child(
            button("search-close", close)
                .on_click(cx.listener(|view, _, window, cx| view.close_search(window, cx)))
                .child(screen.mark(close, icon("icons/close.svg", theme.text))),
        )
    } else {
        // The place shown, as the computer's title bar names it; at All, "All downloads", not the
        // app's name (the maintainer, 2026-11-27: "remove the app name like dm … from homescreen
        // top left corner. make it minimilistic").
        let title =
            crate::window::sidebar::place_name(view).unwrap_or_else(|| t("All downloads").into());
        // The place's name as the bar's title, as a phone's apps name the screen shown; the
        // places are the bottom navigation's, and Add URL the button floating over the list.
        // On a phone, under the title, how the downloads go: the status bar's first words,
        // where a computer shows them along the window's foot.
        // What dm has just said (Exported to …, Backed up dm to …) takes its place for as long
        // as the status bar would show it: a phone has no status bar, and said nothing.
        let summary = crate::platform::ON_A_PHONE
            .then(|| {
                // Not where the Undo bar over the list says it already (gaps row N23).
                (!view.status.undo.is_kept())
                    .then(|| crate::window::statusbar::fresh_said(view))
                    .flatten()
                    .or_else(|| crate::window::statusbar::parts(view).into_iter().next())
            })
            .flatten();
        bar.child(
            drag_space()
                .flex_1()
                .min_w_0()
                .pl(px(space::L))
                .flex()
                .flex_col()
                .justify_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(
                    div()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_size(px(native::TEXT + 5.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(screen.label(title)),
                )
                .when_some(summary, |column, summary| {
                    column.child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_size(px(native::SMALL_TEXT))
                            .text_color(theme.muted)
                            .child(screen.label(gpui::SharedString::from(summary))),
                    )
                }),
        )
        .child(
            button("search-button", filter)
                .on_click(cx.listener(|view, _, window, cx| view.open_search(window, cx)))
                .child(screen.mark(filter, icon("icons/search.svg", theme.text))),
        )
        .child(
            button("overflow-button", menu)
                .on_click(cx.listener(|view, _, _, cx| view.open_overflow(cx)))
                // A drawn mark, not the font's "⋮" (gaps row E41).
                .child(screen.mark(menu, icon("icons/more-vertical.svg", theme.text))),
        )
    };
    bar.children(window_buttons(theme, window.is_maximized()))
}

/// The top bar while several cards are being chosen (gaps row N23): how many, and what can be
/// done to them all.
fn choosing_bar(
    bar: gpui::Stateful<gpui::Div>,
    view: &Downloads,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> gpui::Stateful<gpui::Div> {
    use crate::window::menus::Run;
    use dm_shell::app::commands::Command;
    let button = |id: &'static str, words: &'static str| phone_button(id, words, theme);
    let icon = phone_icon;
    let close = t("Close");
    let all = t("Select all");
    let share = t("Share");
    let delete = t("Delete");
    let count = dm_desktop_base::i18n::tf("{} selected", &[&view.list.selected.len()]);
    // Finished files go to another app, one or several together (gaps row N23): Share where
    // every download chosen is one.
    let chosen = view.chosen();
    let one_file = !chosen.is_empty()
        && chosen
            .iter()
            .all(|r| r.state == dm_core::State::Completed && !r.path.is_empty());
    bar.child(
        button("choosing-close", close)
            .on_click(cx.listener(|view, _, _, cx| {
                view.layout.narrow.choosing = false;
                view.list.selected.clear();
                cx.notify();
            }))
            .child(screen.mark(close, icon("icons/close.svg", theme.text))),
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .text_ellipsis()
            .whitespace_nowrap()
            .text_size(px(native::TEXT + 5.0))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(screen.label(gpui::SharedString::from(count))),
    )
    .child(
        button("choosing-all", all)
            .on_click(cx.listener(|_, _, window, cx| {
                window.dispatch_action(Box::new(Run(Command::SelectAll)), cx);
            }))
            .child(screen.mark(all, icon("icons/place-finished.svg", theme.text))),
    )
    .when(one_file, |bar| {
        bar.child(
            button("choosing-share", share)
                .on_click(cx.listener(|view, _, _, cx| view.share_chosen(cx)))
                .child(screen.mark(share, icon("icons/share.svg", theme.text))),
        )
    })
    .child(
        button("choosing-delete", delete)
            .on_click(cx.listener(|view, _, window, cx| {
                view.layout.narrow.choosing = false;
                window.dispatch_action(Box::new(Run(Command::Remove)), cx);
            }))
            .child(screen.mark(delete, icon("icons/delete.svg", theme.text))),
    )
}

/// Empty space in the bar: where it is the window's title bar, it moves the
/// window and a double-click does the system's setting (zoom or minimize).
/// Not on Linux, under the desktop's own title bar.
fn drag_space() -> gpui::Div {
    div().h_full().when(DRAWN_BY_DM, |space| {
        space
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, |event, window, _| {
                if event.click_count == 2 {
                    window.titlebar_double_click();
                }
            })
    })
}

/// A menu's title in the bar: a click opens its menu under it, as the
/// system draws a menu (an `NSMenu` on macOS, Windows' own, a drawn one on
/// Linux), with the menu bar's items, words and ticks.
fn menu_title(
    index: usize,
    name: SharedString,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    div()
        .id(("menu-title", index))
        .role(Role::MenuItem)
        .aria_label(name.clone())
        .h(px(24.0))
        .px(px(space::M))
        .flex()
        .items_center()
        .rounded(px(native::RADIUS))
        .text_size(px(native::TEXT))
        .hover(|b| b.bg(theme.control))
        // Its own click: not the bar's, whose double-click zooms the window.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(
            cx.listener(move |view, event: &gpui::ClickEvent, window, cx| {
                view.open_window_menu(index, event.position().x - px(10.0), window, cx);
            }),
        )
        .child(screen.label(name))
        // Where it starts, for F10 to open its menu under it.
        .child(
            gpui::canvas(
                move |bounds, _, _| {
                    TITLES_AT.with(|at| {
                        let mut at = at.borrow_mut();
                        if at.len() <= index {
                            at.resize(index + 1, px(0.0));
                        }
                        at[index] = bounds.origin.x;
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
}

thread_local! {
    /// Where each menu title starts in the window, as last drawn.
    static TITLES_AT: std::cell::RefCell<Vec<gpui::Pixels>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Downloads {
    /// The window's menu `index` opened at `x`, under the bar: by a click on its title, or by
    /// F10 for the first, under its title (gaps row U6).
    pub(crate) fn open_window_menu(
        &mut self,
        index: usize,
        x: gpui::Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ticks = self.menus.menu_ticks.clone().unwrap_or_default();
        let Some(menu) = crate::window::menus::window_menus(&ticks)
            .into_iter()
            .nth(index)
        else {
            return;
        };
        self.menus.bar_menu_opened = Some(index);
        // The test platform has no menus of the system's to show.
        if cfg!(test) {
            return;
        }
        NativeMenu::from(menu).show(
            point(x.max(px(0.0)), gpui::px(height()) - px(4.0)),
            window,
            cx,
        );
    }

    /// F10: the first menu, under its title.
    pub(crate) fn menu_bar_by_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let x = TITLES_AT
            .with(|at| at.borrow().first().copied())
            .unwrap_or(px(CONTROLS_AT));
        self.open_window_menu(0, x, window, cx);
    }
}

/// A button of the bar (the side panel's switch, the search shut): its width and height, and
/// its corners, one for both; its picture a toolbar button's (`native::ICON_TOOL`).
const BAR_BUTTON: f32 = 28.0;
const BAR_BUTTON_HEIGHT: f32 = 26.0;
const BAR_CORNER: f32 = native::RADIUS + 1.0;

/// Each of the window's own buttons' width, on Windows: the system's, in
/// points on screen whatever the text size.
const WINDOW_BUTTON: f32 = 46.0;

/// The font Windows draws its own title bars' buttons in: Segoe Fluent
/// Icons on Windows 11, Segoe MDL2 Assets on Windows 10 (the same glyphs at
/// the same places).
fn caption_font() -> &'static str {
    static FONT: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        let fonts = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"))
            .join("Fonts");
        if fonts.join("SegoeIcons.ttf").is_file() {
            "Segoe Fluent Icons"
        } else {
            "Segoe MDL2 Assets"
        }
    })
}

/// Minimize, maximize (restore, while maximized) and close at the bar's
/// right on Windows, as its own programs' title bars have them, in the
/// system's own glyphs: the system takes their clicks (`WindowControlArea`),
/// so Snap layouts open over maximize.
fn window_buttons(theme: Theme, maximized: bool) -> Vec<gpui::AnyElement> {
    if !cfg!(target_os = "windows") {
        return Vec::new();
    }
    [
        ("window-min", "\u{E921}", WindowControlArea::Min),
        (
            "window-max",
            if maximized { "\u{E923}" } else { "\u{E922}" },
            WindowControlArea::Max,
        ),
        ("window-close", "\u{E8BB}", WindowControlArea::Close),
    ]
    .into_iter()
    .map(|(id, glyph, area)| {
        let close = area == WindowControlArea::Close;
        div()
            .id(id)
            .w(gpui::px(WINDOW_BUTTON))
            .h(gpui::px(height()))
            .flex()
            .items_center()
            .justify_center()
            .font_family(caption_font())
            .text_size(gpui::px(10.0))
            // Close turns Windows' red under the pointer, the others grey.
            .hover(move |b| {
                if close {
                    b.bg(gpui::rgb(crate::window::theme::CLOSE_HOVERED))
                        .text_color(gpui::white())
                } else {
                    b.bg(theme.control)
                }
            })
            .window_control_area(area)
            .child(glyph)
            .into_any_element()
    })
    .collect()
}

/// The button that shows or hides the categories at the window's left, as
/// the system's sidebar button: its symbol, a highlight under the pointer.
fn sidebar_button(shown: bool, theme: Theme, cx: &mut Context<Downloads>) -> impl IntoElement {
    use dm_shell::app::commands::Command;
    let words = t(Command::ToggleCategories.label());
    div()
        .id("sidebar-button")
        .role(Role::Button)
        .aria_label(words)
        .aria_toggled(if shown {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .flex_none()
        .w(px(BAR_BUTTON))
        .h(px(BAR_BUTTON_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(BAR_CORNER))
        .hover(|b| b.bg(theme.control))
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(gpui::SharedString::from(words)).build(window, cx)
        })
        // Its own click: not the bar's, whose double-click zooms the window.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|view, _, _, cx| view.run(Command::ToggleCategories, cx)))
        .child(
            svg()
                .path("icons/sidebar.svg")
                .size(px(native::ICON_TOOL))
                // The text's colour, as the toolbar's pictures under it: in the secondary
                // colour it read as greyed.
                .text_color(theme.text),
        )
}
