// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A row's own buttons (Stop, Resume, Open, Details, Delete…), shown at the name's end on the
//! row under the pointer and on a chosen one, in room kept for them (gaps rows U13, E7).

use dm_core::{Entry, State};
use dm_desktop_base::i18n::t;
use dm_shell::app::commands::Command;
use gpui::{div, prelude::*, Context, Role};

use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// The group a row's buttons show with when the pointer is over it.
pub(super) const ROW_GROUP: &str = "row";

/// What a row's own buttons do, by how it stands, shown on the row
/// under the pointer: Stop on one under way, Resume on one paused, Retry on one failed, Open and
/// Open folder on one finished; Details (its progress window) on one not finished; Delete where
/// it can act, on one stopped or finished, never one still writing its file. Each with its
/// picture and the command's words for a screen reader and a script.
pub(super) fn row_actions(row: &Entry) -> Vec<(Command, &'static str, &'static str)> {
    let mut actions = match row.state {
        State::Running | State::Queued => {
            vec![(Command::StopDownload, "icons/stop.svg", t("Stop download"))]
        }
        State::Paused => vec![(Command::DownloadNow, "icons/resume.svg", t("Resume"))],
        State::Failed(_) => vec![(Command::DownloadNow, "icons/resume.svg", t("Retry"))],
        State::Completed => vec![
            (Command::OpenFile, "icons/open.svg", t("Open")),
            (
                Command::OpenFolder,
                "icons/place-folder.svg",
                t("Open folder"),
            ),
        ],
        State::Cancelled => Vec::new(),
    };
    if !matches!(row.state, State::Completed | State::Cancelled) {
        actions.push((Command::ShowProgress, "icons/details.svg", t("Details")));
    }
    if !matches!(row.state, State::Running | State::Queued) {
        actions.push((Command::Remove, "icons/delete.svg", t("Delete")));
    }
    actions
}

/// A row's button's side, and the room between two and around them all.
const ROW_BUTTON: f32 = 18.0;
const ROW_BUTTON_GAP: f32 = 4.0;

/// The room `count` of a row's buttons take at the name's end: the buttons, the gaps between
/// them, their strip's padding and its distance from the cell's edge.
pub(super) fn buttons_width(count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let n = count as f32;
    n * ROW_BUTTON + (n - 1.0) * ROW_BUTTON_GAP + 2.0 * ROW_BUTTON_GAP + ROW_BUTTON_GAP
}

/// A row's buttons, over its right end on `backdrop`: hidden until the pointer is over the row,
/// shown on a chosen one. A press on one acts on that row alone and is the row's no more.
pub(super) fn row_buttons(
    row: &Entry,
    chosen: bool,
    backdrop: gpui::Rgba,
    theme: Theme,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let id = row.id;
    div()
        .absolute()
        .top_0()
        // Over the name's left end where the window is mirrored (gaps row U35).
        .map(|d| {
            if crate::window::mirror::rtl() {
                d.left(px(ROW_BUTTON_GAP))
            } else {
                d.right(px(ROW_BUTTON_GAP))
            }
        })
        .h_full()
        .flex()
        .map(crate::window::mirror::row)
        .items_center()
        .gap(px(ROW_BUTTON_GAP))
        .px(px(ROW_BUTTON_GAP))
        .bg(backdrop)
        .when(!chosen, |d| {
            d.invisible().group_hover(ROW_GROUP, |s| s.visible())
        })
        .children(row_actions(row).into_iter().map(|(command, icon, words)| {
            div()
                .id((command.id(), id as usize))
                .role(Role::Button)
                .aria_label(words)
                .size(px(ROW_BUTTON))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(space::CORNER_S))
                .bg(theme.control)
                // A face answers the pointer a shade toward the words, as every button with a
                // face does (`native::button`); the stripe in its place all but hid it on a
                // chosen row (gaps row E23).
                .hover(move |s| s.bg(theme.answering(theme.control, crate::window::theme::HOVERED)))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _, window, cx| {
                    cx.stop_propagation();
                    // Delete on one of several chosen rows deletes them all; every other
                    // button acts on its own row.
                    if !(command == Command::Remove
                        && view.list.selected.len() > 1
                        && view.list.selected.contains(&id))
                    {
                        view.click_row(id, false, false);
                    }
                    window.dispatch_action(Box::new(crate::window::menus::Run(command)), cx);
                    cx.notify();
                }))
                .child(gpui::svg().path(icon).size(px(12.0)).text_color(theme.text))
        }))
}
