// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A dialog as a window of its own: the operating system's title bar with
//! the dialog's title and its close button, placed over the main window to
//! begin with and moved anywhere on the screen by dragging it, as reference
//! A's are (maintainer, 2026-11-01).
//!
//! Done once, here, for every dialog: `Dialog::show` (`frame.rs`) hands its
//! body to [`show`], which opens it in an egui viewport (as `app/drops.rs`
//! opens the drop target) and sizes the window to what the body measures.
//! Where the backend has no window but the first (the test harness), egui
//! says so (`Context::embed_viewports`) and the dialog stays a window drawn
//! in the main window, as it was.

use super::frame::{lay_out, Layout};
use crate::app::human::ScreenText;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard};

/// Whether dialogs are windows of their own here.
pub(crate) fn separate(ctx: &egui::Context) -> bool {
    !ctx.embed_viewports()
}

/// Whether what is being drawn is one of those windows.
pub(crate) fn in_own_window(ctx: &egui::Context) -> bool {
    separate(ctx) && ctx.viewport_id() != egui::ViewportId::ROOT
}

/// What the main window and the control port need to know of the dialogs'
/// windows, which are drawn one by one inside the main window's frame.
#[derive(Default)]
struct Shared {
    /// Dialogs whose window asked to be closed (its × or Escape) since the
    /// main window last looked.
    closing: Vec<egui::Id>,
    /// The dialog windows drawn this frame, in the order they were.
    open: Vec<(egui::ViewportId, egui::Id)>,
    /// The words each of them drew, for a control client.
    screens: HashMap<egui::ViewportId, Vec<ScreenText>>,
    /// Clicks and keys a control client sent, waiting for the window's next
    /// frame.
    input: HashMap<egui::ViewportId, Vec<egui::Event>>,
    /// The dialog window the system last said had the keyboard.
    focused: Option<egui::ViewportId>,
    /// Dialogs asked for again: brought to the front.
    raise: Vec<egui::Id>,
    /// Whether the main window is minimised, which hides the dialogs with it.
    parent_hidden: bool,
    /// Where each stands on the screen, for a control client.
    info: HashMap<egui::ViewportId, WindowInfo>,
    /// Where the main window stands.
    main: Option<egui::Rect>,
}

static SHARED: LazyLock<Mutex<Shared>> = LazyLock::new(Default::default);

fn shared() -> MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

/// Where a dialog's window stands, as the system last said.
#[derive(Clone, Debug)]
pub struct WindowInfo {
    /// The window's title.
    pub title: String,
    /// With its title bar and frame, in screen points.
    pub outer: Option<egui::Rect>,
    /// The window without its title bar and frame, in screen points.
    pub inner: Option<egui::Rect>,
}

/// What a dialog's window keeps from frame to frame.
#[derive(Clone, Default)]
struct Own {
    /// The size the body last measured, frame and all.
    size: egui::Vec2,
    /// Where it stands: over the main window to begin with, then left where
    /// the person put it.
    pos: Option<egui::Pos2>,
    /// How many frames in a row the size has been the same.
    steady: u8,
    /// Whether it has been put over the main window and is now where the
    /// person leaves it: once, for good. A body that changes size afterwards
    /// (a progress window, a size being read) must not have the window put
    /// back in the middle while it is being dragged.
    placed: bool,
    frames: u32,
    /// Whether the body is taller than the screen and scrolls.
    cramped: bool,
    popup_was_open: bool,
    dark: Option<bool>,
}

/// What a dialog says about its window.
pub(super) struct Spec<'a> {
    pub title: &'a str,
    pub id: egui::Id,
    pub layout: Layout,
    pub resizable: bool,
    pub default_width: Option<f32>,
    /// A height it is given, which its body fills.
    pub height: Option<f32>,
    pub pos: Option<egui::Pos2>,
    pub pivot: egui::Align2,
    pub anchor: Option<(egui::Align2, egui::Vec2)>,
}

/// The start of a pass over the dialogs in the main window.
pub(crate) fn begin_frame(ctx: &egui::Context) {
    let (hidden, main) = ctx.input(|i| {
        (
            i.viewport().minimized == Some(true),
            i.viewport().outer_rect,
        )
    });
    let mut shared = shared();
    shared.parent_hidden = hidden;
    shared.main = main;
    shared.open.clear();
    shared.screens.clear();
    shared.info.clear();
}

/// The dialogs that asked to close, once each.
pub(crate) fn take_closing() -> Vec<egui::Id> {
    std::mem::take(&mut shared().closing)
}

/// Asks for a dialog's window to come to the front.
pub(crate) fn raise(id: egui::Id) {
    shared().raise.push(id);
}

/// The dialog windows drawn this frame, back to front: the one the system last
/// said had the keyboard in front, as it is on screen, then the others in the
/// order they were drawn. A word on two of them is the front one's.
pub(crate) fn windows() -> Vec<(egui::ViewportId, egui::Id)> {
    let shared = shared();
    let mut open = shared.open.clone();
    if let Some(at) = shared
        .focused
        .and_then(|focused| open.iter().position(|(window, _)| *window == focused))
    {
        let front = open.remove(at);
        open.push(front);
    }
    open
}

/// Where the main window and each dialog window stand.
pub(crate) fn placement() -> (Option<egui::Rect>, Vec<(egui::ViewportId, WindowInfo)>) {
    let shared = shared();
    let dialogs = shared
        .open
        .iter()
        .filter_map(|(window, _)| Some((*window, shared.info.get(window)?.clone())))
        .collect();
    (shared.main, dialogs)
}

/// What a dialog window drew this frame.
pub(crate) fn screen_of(viewport: egui::ViewportId) -> Vec<ScreenText> {
    shared().screens.get(&viewport).cloned().unwrap_or_default()
}

/// The window a key or typing from a control client is for: the dialog that
/// has the keyboard, else the front one, else the main window.
pub(crate) fn typing_target() -> egui::ViewportId {
    let shared = shared();
    shared
        .focused
        .filter(|focused| shared.open.iter().any(|(vp, _)| vp == focused))
        .or_else(|| shared.open.last().map(|(vp, _)| *vp))
        .unwrap_or(egui::ViewportId::ROOT)
}

/// Gives a window events for its next frame.
pub(crate) fn queue_input(viewport: egui::ViewportId, events: Vec<egui::Event>) {
    shared().input.entry(viewport).or_default().extend(events);
}

/// Hands a dialog window the events waiting for it, as eframe cannot: it
/// offers the app the first window's input and no other's
/// (`vendor/egui-winit`).
pub fn feed(raw: &mut egui::RawInput) {
    if let Some(events) = shared().input.remove(&raw.viewport_id) {
        raw.events.extend(events);
    }
}

/// Opens (or keeps open) `spec`'s dialog as a window of its own and lays its
/// body out in it. True when the person asked to close it.
pub(super) fn show<R>(
    ctx: &egui::Context,
    spec: Spec<'_>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> bool {
    let Spec {
        title,
        id,
        layout,
        resizable,
        default_width,
        height,
        pos,
        pivot,
        anchor,
    } = spec;
    let viewport = egui::ViewportId::from_hash_of(("dm-dialog-window", id));
    let style = ctx.global_style();
    let pad = style.spacing.window_margin.sum();
    let (client, outer, monitor, parent_hidden) = ctx.input(|i| {
        let view = i.viewport();
        (
            view.inner_rect,
            view.outer_rect,
            view.monitor_size,
            view.minimized == Some(true),
        )
    });
    let monitor = monitor.unwrap_or(egui::vec2(1280.0, 800.0));
    let state_id = id.with("dm-own-window");
    let mut own: Own = ctx
        .data(|d| d.get_temp::<Own>(state_id))
        .unwrap_or_default();

    // What the window may be: as wide as the screen less a margin, and as
    // tall as it less room for the bars; a taller body scrolls.
    let least = (layout.least + pad.x).max(240.0);
    let most = egui::vec2(
        (monitor.x - 40.0).max(least),
        (monitor.y - 120.0).max(200.0),
    );
    let opens_at = layout
        .fixed
        .unwrap_or_else(|| default_width.map_or(least, |w| (w + pad.x).max(least)));
    let mut size = if own.size == egui::Vec2::ZERO {
        egui::vec2(opens_at, 280.0)
    } else {
        own.size
    };
    if let Some(width) = layout.fixed {
        size.x = width;
    }
    size.x = size.x.clamp(least.min(most.x), most.x);
    if let Some(height) = height {
        size.y = height + pad.y;
    }
    size.y = size.y.clamp(80.0, most.y);

    // Placed while it settles to the size its body measures, over the main
    // window; after that it is left where the person moves it.
    let settled = own.placed || own.steady >= 2;
    own.placed = settled;
    if !settled {
        let over = client.or(outer);
        own.pos = over.map(|room| {
            let at = corner(
                room,
                size,
                pos,
                pivot,
                anchor,
                outer.map_or(0.0, |window| window.min.y),
            );
            egui::pos2(at.x, on_screen(at.y, size.y, monitor.y))
        });
    }
    // Shown once its size has settled, so it does not appear at the guess
    // and jump.
    let visible = own.frames >= 3 && !parent_hidden;

    let mut builder = egui::ViewportBuilder::default()
        .with_title(title.to_string())
        .with_inner_size(size)
        .with_min_inner_size(egui::vec2(least.min(size.x), 80.0))
        .with_resizable(resizable)
        .with_minimize_button(false)
        .with_maximize_button(false)
        .with_taskbar(false)
        .with_visible(visible);
    if let Some(at) = own.pos {
        builder = builder.with_position(at);
    }

    let dark = crate::theme::Tokens::get(ctx).dark;
    let raised = {
        let mut shared = shared();
        let before = shared.raise.len();
        shared.raise.retain(|r| *r != id);
        shared.raise.len() != before
    };
    let mut add = Some(add);
    let mut asked = false;
    ctx.show_viewport_immediate(viewport, builder, |vui, _class| {
        let vctx = vui.ctx().clone();
        let close_requested = vctx.input(|i| i.viewport().close_requested());
        // Escape closes the dialog, unless a menu or drop-down had it first.
        let escape = vctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        asked = close_requested || (escape && !own.popup_was_open);
        own.popup_was_open = vctx.any_popup_open();
        if vctx.input(|i| i.viewport().focused) == Some(true) {
            shared().focused = Some(viewport);
        }
        if own.dark != Some(dark) {
            own.dark = Some(dark);
            vctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(if dark {
                egui::SystemTheme::Dark
            } else {
                egui::SystemTheme::Light
            }));
        }
        if raised {
            vctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        let frame = egui::Frame::window(&style)
            .shadow(egui::Shadow::NONE)
            .stroke(egui::Stroke::NONE)
            .corner_radius(0.0);
        let body = Layout {
            tallest: Some(most.y),
            scrolls: layout.scrolls || own.cramped,
            titled: false,
            ..layout
        };
        let mut measured = egui::Vec2::ZERO;
        egui::CentralPanel::default().frame(frame).show(vui, |ui| {
            if let Some(add) = add.take() {
                lay_out(ui, title, &body, add);
            }
            // What the body used: a panel's own rectangle is all it was given
            // before anything is drawn, so it is the layout's cursor that says
            // how tall the body is (less the gap left after its last line).
            measured = egui::vec2(
                ui.min_rect().width(),
                (ui.cursor().top() - ui.max_rect().top() - ui.spacing().item_spacing.y).max(0.0),
            );
        });
        // A pass in which nothing was laid out yet says nothing of the size.
        if measured.y < 1.0 {
            measured.y = (own.size.y - pad.y).max(0.0);
        }
        let mut want = measured + pad;
        want.x = want.x.max(least).min(most.x);
        want.y = want.y.min(most.y);
        if let Some(width) = layout.fixed {
            want.x = width;
        }
        if let Some(height) = height {
            want.y = (height + pad.y).min(most.y);
        }
        if (want - own.size).abs().max_elem() > 0.5 {
            own.size = want;
            own.steady = 0;
        } else {
            own.steady = own.steady.saturating_add(1);
        }
        own.frames = own.frames.saturating_add(1);
        own.cramped = measured.y + pad.y >= most.y - 8.0;
        let seen = crate::app::human::scan_viewport(&vctx, viewport);
        let mut shared = shared();
        shared.screens.insert(viewport, seen);
        shared.info.insert(
            viewport,
            WindowInfo {
                title: title.to_string(),
                outer: vctx.input(|i| i.viewport().outer_rect),
                inner: vctx.input(|i| i.viewport().inner_rect),
            },
        );
        // Once, however many passes the window was drawn in this frame.
        shared.open.retain(|(window, _)| *window != viewport);
        shared.open.push((viewport, id));
    });
    ctx.data_mut(|d| d.insert_temp(state_id, own));
    // The frames that settle it are drawn without waiting for input.
    if !settled {
        ctx.request_repaint();
    }
    asked
}

/// A dialog's window asked to close, as its × or Escape does.
pub(crate) fn ask_close(id: egui::Id) {
    shared().closing.push(id);
}

/// Where a dialog's window stands over `room`, the main window's client
/// area: at `pos` in it with the `pivot` corner of the dialog there, else at
/// the `anchor`, else in the middle; never above `top`, the main window's own
/// top, which a dialog taller than it would otherwise put its title bar out
/// of reach with.
pub(crate) fn corner(
    room: egui::Rect,
    size: egui::Vec2,
    pos: Option<egui::Pos2>,
    pivot: egui::Align2,
    anchor: Option<(egui::Align2, egui::Vec2)>,
    top: f32,
) -> egui::Pos2 {
    let corner = match (pos, anchor) {
        (Some(at), _) => {
            room.min + at.to_vec2()
                - egui::vec2(
                    size.x * pivot.x().to_factor(),
                    size.y * pivot.y().to_factor(),
                )
        }
        (None, Some((align, offset))) => align.align_size_within_rect(size, room).min + offset,
        (None, None) => {
            egui::Align2::CENTER_CENTER
                .align_size_within_rect(size, room)
                .min
        }
    };
    egui::pos2(corner.x, corner.y.max(top))
}

/// The top of a dialog's window, `top`, moved up if its bottom (with the title bar
/// and frame the system draws around the `height` of its body) would be under the
/// taskbar or off a screen `screen` points tall, and never above the screen's own
/// top: a window opened over the middle of a dm window that itself reaches past the
/// bottom of the screen is still reachable, its buttons and all. Only where it
/// opens: after that it is where the person puts it.
pub(crate) fn on_screen(top: f32, height: f32, screen: f32) -> f32 {
    const FRAME: f32 = 40.0;
    const TASKBAR: f32 = 48.0;
    top.min(screen - TASKBAR - (height + FRAME)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_that_would_reach_under_the_taskbar_is_moved_up_and_never_above_the_screen() {
        // 768 tall, a 596-point body: top at 219 would end at 855.
        assert_eq!(on_screen(219.0, 596.0, 768.0), 768.0 - 48.0 - 636.0);
        // One that fits stays where it was.
        assert_eq!(on_screen(120.0, 300.0, 768.0), 120.0);
        // Taller than the screen: its title bar is kept in reach.
        assert_eq!(on_screen(30.0, 900.0, 768.0), 0.0);
    }

    fn room() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(1000.0, 700.0))
    }

    #[test]
    fn a_dialog_opens_over_the_middle_of_the_main_window() {
        let at = corner(
            room(),
            egui::vec2(400.0, 300.0),
            None,
            egui::Align2::LEFT_TOP,
            None,
            0.0,
        );
        assert_eq!(at, egui::pos2(100.0 + 300.0, 50.0 + 200.0));
    }

    #[test]
    fn a_dialog_asked_for_a_place_has_its_pivot_corner_there() {
        // Download complete: its bottom-right corner 16 in and 40 up from the
        // main window's own.
        let size = egui::vec2(300.0, 200.0);
        let at = corner(
            room(),
            size,
            Some(egui::pos2(1000.0 - 16.0, 700.0 - 40.0)),
            egui::Align2::RIGHT_BOTTOM,
            None,
            0.0,
        );
        assert_eq!(at + size, egui::pos2(100.0 + 984.0, 50.0 + 660.0));
    }

    #[test]
    fn a_dialog_with_an_anchor_stands_there() {
        let at = corner(
            room(),
            egui::vec2(400.0, 100.0),
            None,
            egui::Align2::LEFT_TOP,
            Some((egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0))),
            0.0,
        );
        assert_eq!(at, egui::pos2(100.0 + 300.0, 50.0 + 80.0));
    }

    #[test]
    fn a_dialog_never_puts_its_title_bar_above_the_main_window() {
        // Taller than the window: centred it would start above it.
        let at = corner(
            room(),
            egui::vec2(400.0, 900.0),
            None,
            egui::Align2::LEFT_TOP,
            None,
            50.0,
        );
        assert_eq!(at.y, 50.0);
    }

    #[test]
    fn input_for_a_dialog_window_reaches_that_window_once() {
        let first = egui::ViewportId::from_hash_of("first dialog");
        let second = egui::ViewportId::from_hash_of("second dialog");
        queue_input(first, vec![egui::Event::Text("a".into())]);
        queue_input(first, vec![egui::Event::Text("b".into())]);
        queue_input(second, vec![egui::Event::Text("c".into())]);

        let mut other = egui::RawInput {
            viewport_id: second,
            ..Default::default()
        };
        feed(&mut other);
        assert_eq!(other.events, vec![egui::Event::Text("c".into())]);

        let mut raw = egui::RawInput {
            viewport_id: first,
            ..Default::default()
        };
        feed(&mut raw);
        assert_eq!(
            raw.events,
            vec![egui::Event::Text("a".into()), egui::Event::Text("b".into())]
        );
        feed(&mut raw);
        assert_eq!(raw.events.len(), 2, "given once, not again");
    }

    #[test]
    fn the_dialog_window_with_the_keyboard_is_the_front_one() {
        let back = egui::ViewportId::from_hash_of("back window");
        let front = egui::ViewportId::from_hash_of("front window");
        {
            let mut shared = shared();
            shared.open = vec![
                (back, egui::Id::unique("b")),
                (front, egui::Id::unique("f")),
            ];
            shared.focused = Some(back);
        }
        assert_eq!(windows().last().map(|w| w.0), Some(back));
        shared().focused = Some(front);
        assert_eq!(windows().last().map(|w| w.0), Some(front));
        shared().open.clear();
        shared().focused = None;
    }

    #[test]
    fn a_window_asks_to_close_once() {
        let id = egui::Id::unique("a dialog asked to close once");
        ask_close(id);
        let asked = take_closing();
        assert!(asked.contains(&id));
        assert!(!take_closing().contains(&id));
    }
}
