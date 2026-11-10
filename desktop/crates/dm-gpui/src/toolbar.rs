// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The toolbar, a row of its own under the top bar (`titlebar.rs`), as a
//! code editor keeps its bars under its title bar: a picture with its word
//! under it (Finder's "Icon and Text"). The buttons are the application's
//! (`commands::toolbar_shown`: the person's order, less what they hid).

use dm_desktop_base::i18n::t;
use dm_shell::app::commands::Command;
use gpui::{div, prelude::*, px, svg, Context, Role, Window};

use crate::list::Downloads;
use crate::menus::Run;
use crate::screen::Screen;
use crate::theme::Theme;

/// The toolbar's height.
pub const HEIGHT: f32 = 50.0;

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
    // View › Customize toolbar: a picture with its word, the picture alone
    // (its word on hover), or the word alone.
    let style = dm_desktop_base::toolbar::ToolStyle::named(&settings.toolbar_style);
    let picture = style != dm_desktop_base::toolbar::ToolStyle::Text;
    let worded = style != dm_desktop_base::toolbar::ToolStyle::Icons;
    div()
        .id("toolbar")
        .role(Role::Toolbar)
        .aria_label("Toolbar")
        .h(px(HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .border_b_1()
        .border_color(theme.separator)
        .children(buttons.into_iter().map(|(command, words)| {
            let enabled = view.enabled(command);
            let words = t(words);
            // Start Queue and Stop Queue have an arrow beside them that
            // lists the queues; the one chosen is started or stopped,
            // whatever the list shows (B64).
            let queues = matches!(command, Command::StartQueue | Command::StopQueue)
                .then(|| view.backend.queues());
            let arrow = queues.map(|queues| {
                div()
                    .id((command.id(), 1usize))
                    .role(Role::Button)
                    .aria_label(dm_desktop_base::i18n::t("Queues"))
                    .h(px(28.0))
                    .w(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .hover(|s| s.bg(theme.stripe))
                    .child(
                        svg()
                            .path("icons/chevron-up-down.svg")
                            .size(px(10.0))
                            .text_color(theme.muted),
                    )
                    .on_click(move |event, window, cx| {
                        let menu = queues.iter().fold(
                            gpui_component::native_menu::NativeMenu::new(),
                            |menu, queue| {
                                menu.menu(
                                    dm_desktop_base::i18n::name(queue),
                                    Box::new(crate::menus::RunOnQueue {
                                        command,
                                        queue: queue.clone(),
                                    }),
                                )
                            },
                        );
                        menu.show(event.position(), window, cx);
                    })
            });
            let button = div()
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
                // A toolbar item's label: the system's small size.
                .text_size(px(crate::native::SMALL_TEXT))
                .when(!enabled, |b| b.opacity(0.4))
                .when(enabled, |b| {
                    b.hover(|s| s.bg(theme.stripe)).on_click(cx.listener(
                        move |_, _, window, cx| {
                            window.dispatch_action(Box::new(Run(command)), cx);
                        },
                    ))
                })
                .when(picture, |b| {
                    b.child(
                        svg()
                            .path(icon(command))
                            .size(px(18.0))
                            .text_color(theme.text),
                    )
                })
                .when(worded, |b| b.child(screen.label(words)))
                .when(!worded, |b| {
                    b.tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(gpui::SharedString::from(words))
                            .build(window, cx)
                    })
                });
            div().flex().items_center().child(button).children(arrow)
        }))
}
