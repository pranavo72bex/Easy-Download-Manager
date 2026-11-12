// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The floating drop target (View › Drop target), as the egui window's: a
//! small card that stays over every other window, takes a link dragged from
//! a browser or a file dropped on it, shows how far the downloads are along
//! its foot and how fast on hover, moves when dragged, and brings dm's
//! window back on a double-click; its right-click menu has Show dm and Hide
//! the drop target.

use dm_core::State;
use dm_desktop_base::i18n::{t, tn};
use gpui::{
    div, prelude::*, size, Action, App, Bounds, Context, ExternalPaths, MouseButton, Role,
    SharedString, WeakEntity, Window, WindowBounds, WindowKind, WindowOptions,
};
use gpui_component::native_menu::NativeMenu;

use crate::list::Downloads;
use crate::theme::Theme;
use crate::zoom::px;

/// The card's side, in points.
const SIDE: f32 = 44.0;

/// The card's menu: dm's window back.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct ShowDm;

/// The card's menu: the drop target put away (View › Drop target off).
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct HideDropTarget;

/// The drop target's view.
pub struct DropTarget {
    view: WeakEntity<Downloads>,
    /// A link or a file is held over it.
    over: bool,
    /// Its own keyboard, so its menu's choices reach it.
    focus: gpui::FocusHandle,
}

impl DropTarget {
    fn show_dm(&mut self, cx: &mut Context<Self>) {
        let _ = self.view.update(cx, |view, cx| view.show_main(cx));
    }

    fn hide(&mut self, cx: &mut Context<Self>) {
        let _ = self.view.update(cx, |view, cx| {
            view.run(dm_shell::app::commands::Command::ToggleDropTarget, cx)
        });
    }
}

impl Render for DropTarget {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        // How far what is downloading has come, and how fast, for the card
        // to show while dm's window is out of sight.
        let (fraction, busy) = self
            .view
            .upgrade()
            .map(|view| {
                let view = view.read(cx);
                let running: Vec<&dm_core::Entry> = view
                    .rows
                    .iter()
                    .map(|r| &**r)
                    .filter(|r| r.state == State::Running)
                    .collect();
                let total: u64 = running.iter().filter_map(|r| r.progress.total).sum();
                let done: u64 = running
                    .iter()
                    .filter(|r| r.progress.total.is_some())
                    .map(|r| r.progress.downloaded)
                    .sum();
                let fraction = (total > 0).then(|| (done as f32 / total as f32).clamp(0.0, 1.0));
                let busy = (!running.is_empty()).then(|| {
                    let rate: f64 = running.iter().map(|r| r.progress.speed).sum();
                    let said = tn(
                        "{} downloading at {}||{} downloading at {}",
                        running.len() as u64,
                        &[&running.len(), &dm_desktop_base::format::rate(rate)],
                    );
                    match fraction {
                        Some(f) => format!("{said} — {}%", (f * 100.0).floor() as u32),
                        None => said,
                    }
                });
                (fraction, busy)
            })
            .unwrap_or((None, None));
        let tip: SharedString = busy
            .unwrap_or_else(|| t("Drop a link here to download it with dm. Drag to move.").into())
            .into();
        let over = self.over;
        div()
            .id("drop-target")
            .role(Role::Button)
            .aria_label(SharedString::from(t("Drop target")))
            .track_focus(&self.focus)
            .size_full()
            .p(px(3.0))
            .on_action(cx.listener(|this, _: &ShowDm, _, cx| this.show_dm(cx)))
            .on_action(cx.listener(|this, _: &HideDropTarget, _, cx| this.hide(cx)))
            // Dragged by any part of it, as a person moves a small window.
            .on_mouse_down(MouseButton::Left, |event, window, cx| {
                if event.click_count == 2 {
                    return;
                }
                let _ = cx;
                window.start_window_move();
            })
            .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                if event.click_count() == 2 {
                    this.show_dm(cx);
                }
            }))
            .on_mouse_down(MouseButton::Right, |event, window, cx| {
                NativeMenu::new()
                    .menu(t("Show dm"), Box::new(ShowDm))
                    .menu(t("Hide the drop target"), Box::new(HideDropTarget))
                    .show(event.position, window, cx);
            })
            // A file dropped on it; a link from a browser comes through the
            // window's own drag methods (`drops.rs`).
            .on_drag_move(
                cx.listener(|this, _: &gpui::DragMoveEvent<ExternalPaths>, _, cx| {
                    if !this.over {
                        this.over = true;
                        cx.notify();
                    }
                }),
            )
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.over = false;
                let paths = paths.paths().to_vec();
                let _ = this
                    .view
                    .update(cx, |view, cx| view.dropped_files(&paths, cx));
                cx.notify();
            }))
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
            .child(
                div()
                    .relative()
                    .size_full()
                    .rounded(px(8.0))
                    .border_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    // Only something held over it fills it with the accent,
                    // to say that it will take it.
                    .when(over, |d| d.bg(theme.accent).border_color(theme.accent))
                    .when(!over, |d| d.bg(theme.window).border_color(theme.separator))
                    .child(
                        gpui::svg()
                            .path("icons/drop.svg")
                            .size(px(20.0))
                            .text_color(if over { theme.on_accent } else { theme.accent }),
                    )
                    .when_some(fraction, |d, f| {
                        let inner = SIDE - 6.0 - 12.0;
                        d.child(
                            div()
                                .absolute()
                                .left(px(6.0))
                                .bottom(px(5.0))
                                .w(px(inner))
                                .h(px(3.0))
                                .rounded(px(1.5))
                                .bg(theme.separator)
                                .child(
                                    div()
                                        .h_full()
                                        .w(px((inner * f).max(3.0)))
                                        .rounded(px(1.5))
                                        .bg(theme.accent),
                                ),
                        )
                    }),
            )
    }
}

impl Downloads {
    /// The drop target shown or put away as Options says (View › Drop
    /// target), after the update that asked.
    pub(crate) fn follow_drop_target(&mut self, cx: &mut Context<Self>) {
        let wanted = self.backend.settings().drop_target;
        let open = cx.windows();
        let shown = self.drop_window.filter(|w| open.contains(w));
        match (wanted, shown) {
            (true, None) => {
                let view = cx.entity().downgrade();
                let main = self.main_window();
                let me = view.clone();
                cx.defer(move |cx| {
                    let handle = open_window(view, main, cx);
                    let _ = me.update(cx, |me, _| me.drop_window = handle);
                });
                // Not again until it has opened.
                self.drop_window = Some(self.main_window());
            }
            (false, Some(window)) if window != self.main_window() => {
                self.drop_window = None;
                cx.defer(move |cx| {
                    let _ = window.update(cx, |_, window, _| window.remove_window());
                });
            }
            _ => {}
        }
    }
}

/// Opens the drop target, first in the main window's bottom-right corner,
/// above its status bar: a person moves it by dragging.
fn open_window(
    view: WeakEntity<Downloads>,
    main: gpui::AnyWindowHandle,
    cx: &mut App,
) -> Option<gpui::AnyWindowHandle> {
    let corner = main
        .update(cx, |_, window, _| window.bounds())
        .ok()
        .map(|b| {
            gpui::point(
                (b.origin.x + b.size.width - px(SIDE + 20.0)).max(b.origin.x),
                (b.origin.y + b.size.height - px(SIDE + 52.0)).max(b.origin.y),
            )
        })
        .unwrap_or_else(|| gpui::point(px(200.0), px(200.0)));
    let opened = crate::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                corner,
                size(px(SIDE), px(SIDE)),
            ))),
            titlebar: None,
            kind: WindowKind::PopUp,
            focus: false,
            is_resizable: false,
            window_background: gpui::WindowBackgroundAppearance::Transparent,
            ..Default::default()
        },
        move |window, cx| {
            // A link dragged from a browser is taken here too.
            crate::drops::take_links(window);
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            let target = cx.new(|cx| {
                // Its bar follows the downloads.
                if let Some(downloads) = view.upgrade() {
                    cx.observe(&downloads, |_, _, cx| cx.notify()).detach();
                }
                DropTarget {
                    view,
                    over: false,
                    focus,
                }
            });
            cx.new(|cx| gpui_component::Root::new(target, window, cx))
        },
    );
    opened.ok().map(Into::into)
}
