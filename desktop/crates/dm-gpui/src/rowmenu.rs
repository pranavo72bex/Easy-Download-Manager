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
use gpui::{anchored, deferred, div, prelude::*, Context, Pixels, Point, Role};

use crate::list::Downloads;
use crate::menus::Run;
use crate::screen::Screen;
use crate::theme::Theme;
use crate::zoom::px;

/// The menu's items, in groups, each with the words it shows (the egui row
/// menu's own, where it says them differently from the command's title):
/// Resume as `resume` says it for the rows chosen (Retry, Start now).
fn groups(resume: &'static str) -> Vec<Vec<(Command, &'static str)>> {
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
            (C::DownloadNow, t(resume)),
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
    let resume = dm_desktop_base::rows::resume_word(&view.chosen());
    for (g, group) in groups(resume).into_iter().enumerate() {
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
    // "Delete file after", as the egui row menu had it: the file goes to the Trash, its row
    // with it, that long after it finished; Never stops the countdown (gaps row U8). For the
    // rows that have, or will have, a file: not a failed or cancelled one.
    let targets: Vec<dm_core::Id> = view
        .chosen()
        .into_iter()
        .filter(|r| {
            !matches!(
                r.state,
                dm_core::State::Failed(_) | dm_core::State::Cancelled
            )
        })
        .map(|r| r.id)
        .collect();
    if !targets.is_empty() {
        let chosen = view.chosen();
        let shared = chosen
            .first()
            .map(|r| r.delete_after)
            .filter(|first| chosen.iter().all(|r| r.delete_after == *first));
        items.push(
            div()
                .mx_2()
                .my_1()
                .h(px(1.0))
                .bg(theme.separator)
                .into_any_element(),
        );
        items.push(
            div()
                .mx_1()
                .px_2()
                .h(px(20.0))
                .flex()
                .items_center()
                .text_size(px(crate::native::SMALL_TEXT))
                .text_color(theme.muted)
                .child(screen.label(t("Delete file after")))
                .into_any_element(),
        );
        for (words, seconds) in dm_desktop_base::file_info::delete_after_times() {
            let targets = targets.clone();
            let on = shared == Some(seconds);
            items.push(
                div()
                    .id(("delete-after", seconds.unwrap_or(0)))
                    .role(Role::MenuItem)
                    .aria_label(words)
                    .mx_1()
                    .px_2()
                    .h(px(22.0))
                    .rounded(px(4.0))
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(crate::native::TEXT))
                    .hover(|s| s.bg(theme.selection).text_color(theme.on_selection))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.close_menu();
                        for &id in &targets {
                            view.backend.set_delete_after(id, seconds);
                        }
                        view.read();
                        cx.notify();
                    }))
                    .child(div().w(px(14.0)).child(if on { "✓" } else { "" }))
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
                // The menu takes its own clicks: a row under it took them too,
                // and Extract here, over another row, chose that row first and
                // extracted it instead (Linux, real input, 2026-11-11).
                .occlude()
                .on_mouse_down_out(cx.listener(|view, _, _, cx| {
                    view.close_menu();
                    cx.notify();
                }))
                .children(items),
        ),
    )
    .with_priority(1)
}
