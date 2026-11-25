// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A bottom sheet, as a phone's own are, the one every sheet of dm's is drawn by (the
//! maintainer, 2026-11-21: "the dialog open when click to 3 dots options should be drag down by
//! finger … add close (X) icon … apply in all bottomsheet dialog in application"): across the
//! screen's foot, over a dimmed window whose tap closes it, with a handle, its name and a close
//! button at its top, and its items under them, scrolling where they are more than the screen
//! holds. A finger dragging its top down moves it with the finger; let go more than
//! [`CLOSE_AFTER`] down it closes, else it settles back. The close button, and a name that is
//! itself a button (a submenu's, which goes back), keep their taps; the handle's strip always
//! drags.
//!
//! The sheet's owner keeps where it is (a [`Drag`]) and answers through the [`Hooks`]: the
//! overflow menu and a card's menu keep theirs in the list's view (`window/narrow/sheet.rs`),
//! a pop-up's choices on a phone in the pop-up's own state (`popup.rs`).

use std::rc::Rc;

use gpui::{div, prelude::*, AnyElement, App, Div, Role, SharedString, Stateful, Window};

use crate::widgets::{native, space};
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// How far down a sheet is dragged, in points, for it to close when let go.
pub(crate) const CLOSE_AFTER: f32 = 80.0;

/// The strip at a sheet's top that holds its handle, in points: always a place to drag it by.
pub(crate) const HANDLE_STRIP: f32 = 24.0;

/// A sheet item's height: a finger's target.
pub(crate) const ITEM_HEIGHT: f32 = space::TOUCH;

/// Where a sheet is: how far down a finger has dragged it, and whether one is dragging it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Drag {
    /// How far down, in points.
    pub(crate) down: f32,
    /// Whether a finger is dragging it by its top.
    pub(crate) dragging: bool,
}

/// Closes a sheet.
pub(crate) type Close = Rc<dyn Fn(&mut Window, &mut App)>;
/// Puts a sheet where a finger has dragged it.
pub(crate) type SetDrag = Rc<dyn Fn(Drag, &mut Window, &mut App)>;

/// What a sheet asks of its owner, while drawn and as a finger drags it.
#[derive(Clone)]
pub(crate) struct Hooks {
    /// Closes it.
    pub(crate) close: Close,
    /// Where it is now, read as a finger moves (the drawn frame's may be older).
    pub(crate) drag: Rc<dyn Fn(&App) -> Drag>,
    /// Puts it there.
    pub(crate) set_drag: SetDrag,
}

/// One line of a sheet, a finger's height.
pub(crate) fn line(id: impl Into<gpui::ElementId>, theme: Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(ITEM_HEIGHT))
        .px(px(space::L))
        .flex()
        .items_center()
        .gap(px(space::M))
        .text_size(px(native::TEXT))
        // Reached by Tab with its ring (gaps row E25), and answering the pointer as a
        // control's face does: `theme.control` was 1.08 to 1 on a menu.
        .map(|l| native::ringed(l, theme))
        .rounded_none()
        .hover(move |l| l.bg(theme.answering(theme.menu, crate::window::theme::HOVERED)))
}

/// The dimmed layer over the whole window a sheet stands on; a tap on it, outside the sheet,
/// closes the sheet.
pub(crate) fn scrim(id: impl Into<gpui::ElementId>, hooks: &Hooks) -> Stateful<Div> {
    let close = hooks.close.clone();
    div()
        .id(id)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .bg(gpui::rgba(crate::window::theme::SCRIM))
        .occlude()
        .on_click(move |_, window, cx| close(window, cx))
}

/// The sheet: `name` at its top (a button of its own where `name_taps`), then `items`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sheet(
    id: impl Into<gpui::ElementId>,
    label: SharedString,
    name: Stateful<Div>,
    name_taps: bool,
    items: Vec<AnyElement>,
    drag: Drag,
    hooks: &Hooks,
    theme: Theme,
    screen: &Screen,
    window: &Window,
) -> Stateful<Div> {
    let viewport = window.viewport_size();
    let safe = crate::window::safe::edges(window);
    // Up to the top bar's foot at most, clear of the phone's bars.
    let top = crate::window::titlebar::narrow_height() + f32::from(safe.top);
    let close_words = dm_desktop_base::i18n::t("Close");
    let close = hooks.close.clone();
    let top_part = div()
        .id("sheet-top")
        .relative()
        .flex_none()
        .flex()
        .flex_col()
        .border_b_1()
        .border_color(theme.separator)
        .child(
            div()
                .w_full()
                .h(px(HANDLE_STRIP))
                .flex()
                .justify_center()
                .items_center()
                .child(div().w(px(36.0)).h(px(4.0)).rounded_full().bg(theme.muted)),
        )
        .child(
            div()
                .pl(px(space::L))
                .pr(px(space::XS))
                .flex()
                .items_center()
                .gap(px(space::S))
                .text_size(px(native::TEXT + 2.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(name.flex_1().min_w_0())
                .child(
                    div()
                        .id("sheet-close")
                        .role(Role::Button)
                        .aria_label(close_words)
                        .flex_none()
                        .size(px(space::TOUCH))
                        .flex()
                        .items_center()
                        .justify_center()
                        .map(|b| native::ringed(b, theme))
                        .rounded_full()
                        .active(move |b| b.bg(theme.control))
                        .on_click(move |_, window, cx| close(window, cx))
                        .child(screen.mark(close_words, div().text_color(theme.muted).child("✕"))),
                ),
        )
        .child(drag_area(name_taps, hooks.clone()));
    div()
        .id(id)
        .role(Role::Menu)
        .aria_label(label)
        .absolute()
        .bottom(gpui::px(-drag.down.max(0.0)))
        .left_0()
        .right_0()
        .max_h(viewport.height - gpui::px(top))
        .pb(px(space::M) + safe.bottom)
        .pl(safe.left)
        .pr(safe.right)
        .flex()
        .flex_col()
        .rounded_t(px(space::CORNER_L * 2.0))
        .border_t_1()
        .border_color(theme.separator)
        .bg(theme.menu)
        .text_color(theme.text)
        .occlude()
        .child(top_part)
        .child(
            div()
                .id("sheet-items")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pt(px(space::XS))
                .flex()
                .flex_col()
                .children(items),
        )
}

/// The sheet's top as a place a finger drags it down by.
fn drag_area(name_taps: bool, hooks: Hooks) -> impl IntoElement {
    gpui::canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let hooks = hooks.clone();
            window.on_mouse_event(move |event: &gpui::TouchDragEvent, phase, window, cx| {
                if phase != gpui::DispatchPhase::Bubble {
                    return;
                }
                let started = event.phase == gpui::TouchPhase::Started;
                if started {
                    let at = event.start_position;
                    // The close button keeps its tap, and a name that is a button keeps its;
                    // the handle's strip always drags.
                    let on_close = at.x > bounds.right() - px(space::TOUCH + space::XS);
                    let on_handle = at.y < bounds.top() + px(HANDLE_STRIP);
                    if !bounds.contains(&at) || (!on_handle && (on_close || name_taps)) {
                        return;
                    }
                    (hooks.set_drag)(
                        Drag {
                            down: 0.0,
                            dragging: true,
                        },
                        window,
                        cx,
                    );
                    window.prevent_default();
                    return;
                }
                if !(hooks.drag)(cx).dragging {
                    return;
                }
                let down = f32::from(event.position.y - event.start_position.y).max(0.0);
                match event.phase {
                    gpui::TouchPhase::Ended if down > CLOSE_AFTER => (hooks.close)(window, cx),
                    gpui::TouchPhase::Ended | gpui::TouchPhase::Cancelled => {
                        (hooks.set_drag)(Drag::default(), window, cx)
                    }
                    _ => (hooks.set_drag)(
                        Drag {
                            down,
                            dragging: true,
                        },
                        window,
                        cx,
                    ),
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
