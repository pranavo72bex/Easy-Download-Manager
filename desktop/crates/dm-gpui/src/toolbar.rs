// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The toolbar, where macOS programs keep theirs: in the window's title bar,
//! beside the traffic lights, a picture with its word under it (Finder's
//! "Icon and Text"). The buttons are the application's
//! (`commands::toolbar_shown`: the person's order, less what they hid);
//! the bar's empty space moves the window, and a double-click on it does
//! what the system's settings say (zoom or minimize).

use dm_desktop_base::i18n::t;
use dm_shell::app::commands::Command;
use gpui::{div, prelude::*, px, svg, Context, MouseButton, Role, Window, WindowControlArea};

use crate::list::Downloads;
use crate::menus::{carried_out, Run};
use crate::screen::Screen;
use crate::theme::Theme;

/// The title bar's height with the toolbar in it.
pub const HEIGHT: f32 = 52.0;

/// Room for the traffic lights at the left.
const TRAFFIC_LIGHTS: f32 = 78.0;

/// The picture on a toolbar command's button.
pub fn icon(command: Command) -> &'static str {
    match command {
        Command::AddUrl => "icons/add.svg",
        Command::DownloadNow => "icons/resume.svg",
        Command::StopDownload => "icons/stop.svg",
        Command::StopAll => "icons/stop-all.svg",
        Command::Remove => "icons/delete.svg",
        Command::DeleteCompleted => "icons/delete-completed.svg",
        Command::Options => "icons/options.svg",
        Command::Scheduler => "icons/scheduler.svg",
        Command::StartQueue => "icons/start-queue.svg",
        Command::StopQueue => "icons/stop-queue.svg",
        _ => "icons/grabber.svg",
    }
}

/// The toolbar.
pub fn toolbar(
    view: &Downloads,
    theme: Theme,
    screen: &Screen,
    _window: &mut Window,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let settings = view.backend.settings();
    let buttons = dm_shell::app::commands::toolbar_shown(&settings);
    div()
        .id("toolbar")
        .role(Role::Toolbar)
        .aria_label("Toolbar")
        .h(px(HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .pl(px(TRAFFIC_LIGHTS))
        .pr_2()
        .border_b_1()
        .border_color(theme.separator)
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.titlebar_double_click();
            }
        })
        .children(buttons.into_iter().map(|(command, words)| {
            let enabled = carried_out(command);
            let words = t(words);
            div()
                .id(command.id())
                .role(Role::Button)
                .aria_label(words)
                .flex()
                .flex_col()
                .items_center()
                .gap(px(1.0))
                .px_2()
                .py_1()
                .rounded_md()
                .text_xs()
                .when(!enabled, |b| b.opacity(0.4))
                .when(enabled, |b| {
                    b.hover(|s| s.bg(theme.stripe)).on_click(cx.listener(
                        move |_, _, window, cx| {
                            window.dispatch_action(Box::new(Run(command)), cx);
                        },
                    ))
                })
                .child(
                    svg()
                        .path(icon(command))
                        .size(px(18.0))
                        .text_color(theme.text),
                )
                .child(screen.label(words))
        }))
}
