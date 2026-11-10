// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The row menu, where a right-click (or Control-click) lands, drawn as a
//! macOS context menu: rounded and shadowed, the item under the pointer in
//! the selection colour, what does not apply to the rows chosen greyed. Its
//! items and their words are the egui window's row menu's.

use dm_desktop_base::i18n::t;
use dm_shell::app::commands::Command;
use gpui::{anchored, deferred, div, prelude::*, px, Context, Pixels, Point, Role};

use crate::list::Downloads;
use crate::menus::Run;
use crate::screen::Screen;
use crate::theme::Theme;

/// The menu's items, in groups, each with the words it shows (the egui row
/// menu's own, where it says them differently from the command's title).
fn groups() -> Vec<Vec<(Command, &'static str)>> {
    use Command as C;
    let title = |c: Command| t(c.label());
    vec![
        vec![
            (C::OpenFile, title(C::OpenFile)),
            (C::OpenWith, title(C::OpenWith)),
            (C::Preview, title(C::Preview)),
            (C::OpenFolder, title(C::OpenFolder)),
            (C::ExtractHere, title(C::ExtractHere)),
            (C::ExtractTo, title(C::ExtractTo)),
            (C::MoveRename, title(C::MoveRename)),
            (C::Redownload, title(C::Redownload)),
        ],
        vec![
            (C::DownloadNow, title(C::DownloadNow)),
            (C::StopDownload, t("Stop download")),
            (C::UpdateLink, t("Update download link")),
            (C::CopyLink, t("Copy link")),
            (C::CopyDetails, t("Copy details")),
            (C::Remove, t("Delete")),
            (C::DeleteFile, title(C::DeleteFile)),
        ],
        vec![
            (C::MoveToQueue, t("Move to queue")),
            (C::RemoveFromQueue, title(C::RemoveFromQueue)),
            (C::MoveUp, title(C::MoveUp)),
            (C::MoveDown, title(C::MoveDown)),
        ],
        vec![(C::Properties, t("Properties"))],
    ]
}

/// The row menu, open at `at` (window points).
pub fn menu(
    view: &Downloads,
    at: Point<Pixels>,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let mut items = Vec::new();
    for (g, group) in groups().into_iter().enumerate() {
        if g > 0 {
            items.push(
                div()
                    .mx_2()
                    .my_1()
                    .h(px(1.0))
                    .bg(theme.separator)
                    .into_any_element(),
            );
        }
        for (command, words) in group {
            let enabled = view.enabled(command);
            items.push(
                div()
                    .id(command.id())
                    .role(Role::MenuItem)
                    .aria_label(words)
                    .mx_1()
                    .px_2()
                    .h(px(22.0))
                    .rounded(px(4.0))
                    .flex()
                    .items_center()
                    .text_size(px(crate::native::TEXT))
                    .when(!enabled, |d| d.text_color(theme.muted).opacity(0.6))
                    .when(enabled, |d| {
                        d.hover(|s| s.bg(theme.selection).text_color(theme.on_selection))
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.close_menu();
                                window.dispatch_action(Box::new(Run(command)), cx);
                                cx.notify();
                            }))
                    })
                    .child(screen.label(words))
                    .into_any_element(),
            );
        }
    }
    deferred(
        anchored().position(at).child(
            div()
                .id("row-menu")
                .role(Role::Menu)
                .aria_label("Row menu")
                .min_w(px(220.0))
                .py_1()
                .rounded(px(6.0))
                .border_1()
                .border_color(theme.separator)
                .bg(theme.menu)
                .shadow_lg()
                .text_color(theme.text)
                .on_mouse_down_out(cx.listener(|view, _, _, cx| {
                    view.close_menu();
                    cx.notify();
                }))
                .children(items),
        ),
    )
    .with_priority(1)
}
