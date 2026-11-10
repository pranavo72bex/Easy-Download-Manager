// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's top bar, as a code editor's title bar holds its command
//! centre (the maintainer's word of 2026-11-10: "take reference from
//! vscode"): the system's traffic lights at the left and, in the middle of
//! the window, the search box that narrows the list as one types (its name,
//! address or note, as the egui window's top-row box), how many rows it
//! leaves and a way to clear it once it holds words. Beside the traffic
//! lights, the button that shows or hides the categories, where macOS's own
//! programs put their sidebar's (⌃⌘S as well). The bar's empty space moves
//! the window; a double-click on it does what the system's settings say
//! (zoom or minimize).

use dm_desktop_base::i18n::{t, tn};
use gpui::{div, prelude::*, px, svg, Context, MouseButton, Role, Window, WindowControlArea};
use gpui_component::input::Input;

use crate::list::Downloads;
use crate::native;
use crate::screen::Screen;
use crate::theme::Theme;

/// The top bar's height: the traffic lights centred in it.
pub const HEIGHT: f32 = 38.0;

/// Where the system draws the traffic lights, in the bar.
pub const TRAFFIC_LIGHTS_AT: (f32, f32) = (14.0, 12.0);

/// Where the sidebar's button stands: just past the traffic lights.
const SIDEBAR_BUTTON_AT: f32 = 78.0;

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
    let width = (f32::from(window.viewport_size().width) * 0.4).clamp(220.0, BOX_WIDEST);
    div()
        .id("titlebar")
        .role(Role::Group)
        .aria_label(t("Filter"))
        .h(px(HEIGHT))
        .flex_none()
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.sidebar)
        .border_b_1()
        .border_color(theme.separator)
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.titlebar_double_click();
            }
        })
        .child(sidebar_button(view.show_sidebar, theme, cx))
        .child(
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
                }),
        )
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
        .absolute()
        .left(px(SIDEBAR_BUTTON_AT))
        .top(px((HEIGHT - 24.0) / 2.0))
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
