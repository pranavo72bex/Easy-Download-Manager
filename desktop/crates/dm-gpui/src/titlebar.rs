// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's top bar, as a code editor's title bar holds its menus and
//! command centre (the maintainer's words of 2026-11-10: "take reference
//! from vscode", "make the search bar and options like Tasks File Downloads
//! View Help to top bar", on macOS, Windows and Linux): the system's traffic
//! lights at the left on macOS; the button that shows or hides the
//! categories (⌃⌘S as well); the menus' titles, Tasks, File, Downloads,
//! View and Help, each opening its menu as the system draws one
//! (`menus::window_menus`); and, centred in the room left, the search box
//! that narrows the list as one types (its name, address or note, as the
//! egui window's top-row box), how many rows it leaves and a way to clear
//! it. On Windows the bar ends with the window's own buttons. The bar's
//! empty space moves the window; a double-click on it does what the
//! system's settings say (zoom or minimize).

use dm_desktop_base::i18n::{t, tn};
use gpui::{
    div, point, prelude::*, px, svg, Context, MouseButton, Role, SharedString, Window,
    WindowControlArea,
};
use gpui_component::input::Input;
use gpui_component::native_menu::NativeMenu;

use crate::list::Downloads;
use crate::native;
use crate::screen::Screen;
use crate::theme::Theme;

/// Whether the top bar is the window's title bar (macOS, Windows), or a
/// row under the desktop's own (Linux, where a program drawing its title
/// bar would draw its own frame and edges too).
pub const DRAWN_BY_DM: bool = cfg!(any(target_os = "macos", windows));

/// The top bar's height: the traffic lights centred in it.
pub const HEIGHT: f32 = 38.0;

/// Where the system draws the traffic lights, in the bar.
pub const TRAFFIC_LIGHTS_AT: (f32, f32) = (14.0, 12.0);

/// Where the bar's own controls begin: past the traffic lights on macOS,
/// at the window's edge elsewhere.
const CONTROLS_AT: f32 = native::pick(78.0, 8.0, 8.0);

thread_local! {
    /// Where the menus' titles end in the window, as the last frame laid
    /// them out (their words are the language's, their size the system's):
    /// the search box starts after it.
    static MENUS_END: std::cell::Cell<f32> = const { std::cell::Cell::new(CONTROLS_AT + 340.0) };
}

/// The search box's height and its widest.
const BOX_HEIGHT: f32 = 26.0;
const BOX_WIDEST: f32 = 520.0;

/// The top bar.
pub fn titlebar(
    view: &Downloads,
    theme: Theme,
    screen: &Screen,
    window: &mut Window,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let words = view.filter.read(cx).value().to_string();
    let searching = !words.trim().is_empty();
    let shown = view.shown.len();
    // As wide as two fifths of the window, between a field's least and the
    // widest a title bar's box goes, centred on the window whatever stands
    // at its sides.
    let viewport = f32::from(window.viewport_size().width);
    let menus_end = MENUS_END.with(std::cell::Cell::get);
    let room = viewport - menus_end - WINDOW_BUTTONS_WIDTH - 16.0;
    let width = (viewport * 0.4)
        .clamp(220.0, BOX_WIDEST)
        .min(room.max(140.0));
    // Centred on the window where the menus leave room, as the editor's
    // command centre is; else in the middle of the room after them.
    let centred = (viewport - width) / 2.0;
    let before = (centred >= menus_end + 8.0).then_some(centred - menus_end);
    div()
        .id("titlebar")
        .role(Role::Group)
        .aria_label(t("Filter"))
        .h(px(HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .bg(theme.sidebar)
        .border_b_1()
        .border_color(theme.separator)
        .child(drag_space().w(px(CONTROLS_AT)).flex_none())
        .child(sidebar_button(view.show_sidebar, theme, cx))
        .child(
            div()
                .id("menu-titles")
                .role(Role::MenuBar)
                .aria_label(t("Menu"))
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .ml(px(4.0))
                .children(
                    crate::menus::WINDOW_MENUS
                        .iter()
                        .enumerate()
                        .map(|(i, name)| menu_title(i, t(name).into(), theme, screen, cx)),
                )
                // Where they end, for the next frame's search box.
                .child(
                    gpui::canvas(
                        |_, _, _| {},
                        |bounds, _, window, _| {
                            let end = f32::from(bounds.origin.x + bounds.size.width);
                            if (MENUS_END.with(std::cell::Cell::get) - end).abs() > 0.5 {
                                MENUS_END.with(|e| e.set(end));
                                window.refresh();
                            }
                        },
                    )
                    .absolute()
                    .size_full(),
                ),
        )
        // The empty space each side of the search box moves the window; the
        // controls take their own clicks.
        .child(match before {
            Some(gap) => drag_space().w(px(gap)).flex_none(),
            None => drag_space().flex_1(),
        })
        .child(search_box(view, width, searching, shown, theme, screen, cx))
        .child(drag_space().flex_1())
        .children(window_buttons(theme))
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

/// The search box.
fn search_box(
    view: &Downloads,
    width: f32,
    searching: bool,
    shown: usize,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    div()
        .id("search")
        // Escape in the box clears it and gives the keyboard back to
        // the list, as the egui window's box does.
        .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                view.clear_filter(window, cx);
                window.focus(&view.focus, cx);
                cx.stop_propagation();
            }
        }))
        .w(px(width))
        .h(px(BOX_HEIGHT))
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(8.0))
        .rounded(px(native::RADIUS + 1.0))
        .border_1()
        .border_color(theme.separator)
        .bg(theme.stripe)
        // The magnifier at the left, as the system's search fields
        // and the editor's command centre draw it.
        .child(
            svg()
                .path("icons/search.svg")
                .size(px(13.0))
                .flex_none()
                .text_color(theme.muted),
        )
        .child(
            div().flex_1().min_w_0().child(
                gpui::Styled::h(
                    Input::new(&view.filter)
                        .appearance(false)
                        .aria_label(t("Filter")),
                    px(BOX_HEIGHT - 2.0),
                )
                .text_size(px(native::TEXT))
                .px(px(0.0)),
            ),
        )
        // How many rows the words leave, and a way to clear them.
        .when(searching, |d| {
            d.child(
                div()
                    .flex_none()
                    .text_size(px(native::SMALL_TEXT))
                    .text_color(theme.muted)
                    .child(screen.label(tn("{} shown||{} shown", shown as u64, &[&shown]))),
            )
            .child(
                div()
                    .id("clear-filter")
                    .role(Role::Button)
                    .aria_label(t("Clear"))
                    .flex_none()
                    .size(px(16.0))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme.muted)
                    .text_color(theme.window)
                    .text_size(px(10.0))
                    .child(screen.label("✕"))
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.clear_filter(window, cx);
                    })),
            )
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
        .px(px(7.0))
        .flex()
        .items_center()
        .rounded(px(native::RADIUS))
        .text_size(px(native::TEXT))
        .hover(|b| b.bg(theme.control))
        // Its own click: not the bar's, whose double-click zooms the window.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(
            cx.listener(move |view, event: &gpui::ClickEvent, window, cx| {
                let ticks = view.menu_ticks.clone().unwrap_or_default();
                if let Some(menu) = crate::menus::window_menus(&ticks).into_iter().nth(index) {
                    let x = event.position().x - px(10.0);
                    NativeMenu::from(menu).show(
                        point(x.max(px(0.0)), px(HEIGHT - 4.0)),
                        window,
                        cx,
                    );
                }
            }),
        )
        .child(screen.label(name))
}

/// The room the window's own buttons take: on Windows, where the bar is the
/// window's title bar; elsewhere the system draws them.
const WINDOW_BUTTONS_WIDTH: f32 = native::pick(0.0, 3.0 * WINDOW_BUTTON, 0.0);

/// Each of the window's own buttons' width, on Windows.
const WINDOW_BUTTON: f32 = 46.0;

/// Minimize, maximize and close at the bar's right on Windows, as its own
/// programs' title bars have them: the system takes their clicks
/// (`WindowControlArea`), so Snap layouts open over maximize.
fn window_buttons(theme: Theme) -> Vec<gpui::AnyElement> {
    if !cfg!(target_os = "windows") {
        return Vec::new();
    }
    [
        ("window-min", "—", WindowControlArea::Min),
        ("window-max", "☐", WindowControlArea::Max),
        ("window-close", "✕", WindowControlArea::Close),
    ]
    .into_iter()
    .map(|(id, glyph, area)| {
        let close = area == WindowControlArea::Close;
        div()
            .id(id)
            .w(px(WINDOW_BUTTON))
            .h(px(HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(native::SMALL_TEXT))
            // Close turns Windows' red under the pointer, the others grey.
            .hover(move |b| {
                if close {
                    b.bg(gpui::rgb(0xc42b1c)).text_color(gpui::white())
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
        .w(px(30.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(native::RADIUS))
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
                .size(px(17.0))
                .text_color(theme.muted),
        )
}
