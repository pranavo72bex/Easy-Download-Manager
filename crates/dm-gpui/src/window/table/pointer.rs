// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a row of the download table does with the pointer (`rows`).

use super::*;

/// Row `id`'s handlers on `r`: its menu on a right-click, a click choosing it and a
/// double-click opening it, its file `dragged` out, and, where the list `reorders`, a row
/// dropped on it taking its place, marked in `original`'s accent.
pub(super) fn row_handlers(
    r: Stateful<Div>,
    id: dm_core::Id,
    dragged: Option<crate::platform::dragout::DraggedRow>,
    reorders: bool,
    original: Theme,
    cx: &mut Context<Downloads>,
) -> Stateful<Div> {
    r.on_mouse_down(
        gpui::MouseButton::Right,
        cx.listener(move |view, event: &gpui::MouseDownEvent, window, cx| {
            view.open_menu(id, event.position, window, cx);
            cx.notify();
        }),
    )
    .on_click(
        cx.listener(move |view, event: &gpui::ClickEvent, window, cx| {
            // The primary button's alone: GPUI hands the
            // right button's click here too, after its press
            // opens the menu above, and taken, it would choose
            // the row by itself, so Move/Rename for two chosen
            // rows would rename one (Linux, real input).
            if !event.standard_click() {
                return;
            }
            // A Control-click is a right-click on macOS.
            if event.modifiers().control && cfg!(target_os = "macos") {
                if let Some(at) = event.mouse_position() {
                    view.open_menu(id, at, window, cx);
                    cx.notify();
                    return;
                }
            }
            let held = event.modifiers();
            view.click_row(id, held.platform || held.control, held.shift);
            if event.click_count() == 2 {
                view.double_click(id, cx);
            }
            cx.notify();
        }),
    )
    // A finished row's file dragged out of the window
    // (`dragout`); a click without moving still chooses
    // the row, and a double-click opens it.
    .when_some(dragged, |r, dragged| {
        r.on_drag(dragged, |dragged, _, _, cx| dragged.picture(cx))
            .external_drag_payload(|dragged: &crate::platform::dragout::DraggedRow, _, cx| {
                dragged.payload(cx)
            })
    })
    // A row dropped here takes this row's place (gaps row U39).
    .when(reorders, |r| {
        r.drag_over::<crate::platform::dragout::DraggedRow>(move |style, _, _, _| {
            style.border_t_2().border_color(original.accent_text)
        })
        .on_drop(cx.listener(
            move |view, dragged: &crate::platform::dragout::DraggedRow, _, cx| {
                view.move_row(dragged.id, id);
                cx.notify();
            },
        ))
    })
}
