// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The side panel, as a macOS source list (Finder's, Mail's): a heading for
//! each group, each place with how many it holds, the chosen one drawn in
//! the system's selection colour. The places, their counts and their order
//! are the egui window's (`dm_shell::app::places`): All Downloads (All,
//! each kind, a person's own categories), Unfinished, Finished, and Queues.
//! Its right edge dragged makes it wider or narrower, kept across launches
//! (`Settings::sidebar_width`), and a name longer than it has room for is cut
//! with an ellipsis and read whole on hover (gaps row U12).

use dm_core::Category;
use dm_desktop_base::i18n::{name, t};
use dm_shell::app::places::Place;
use dm_shell::settings::SIDEBAR_WIDTH;
use gpui::{div, prelude::*, Context, Role, SharedString, Window};

use crate::list::Downloads;
use crate::screen::Screen;
use crate::theme::Theme;
use crate::zoom::px;

/// How wide the strip at the panel's right edge is that a press there drags, in points.
const EDGE: f32 = 5.0;

/// What a place's row gives to all but its name, in points: its margins and padding (`mx_2`,
/// `px_2`: 8 each side), its picture (16) and the gap after it (8), and a gap before its count.
const ROW_BESIDE_NAME: f32 = 16.0 + 16.0 + 16.0 + 8.0 + 8.0;

/// Whether a name `name` pixels wide, beside a count `count` pixels wide, is cut in a panel
/// `wide` points across (at 100%) at the scale `factor`: then it is offered whole in a tooltip.
/// The widths are the laid-out text's (`drawn_width`), not guessed from its letters: a guess
/// missed names of wide letters (capitals, a W or an M, some scripts) that the panel cut. A
/// pixel's leeway, and the panel's 1-pixel edge line, err toward offering it.
fn cut(name: f32, count: f32, wide: f32, factor: f32) -> bool {
    let room = (wide - ROW_BESIDE_NAME) * factor - 1.0 - count;
    name > room - 1.0
}

/// How wide `text` is drawn at `size` points (at 100%) in the window's interface font, in pixels
/// on screen: laid out by the system's text shaper, which keeps what it laid out from one frame
/// to the next, so a panel's dozen names cost little.
pub(crate) fn drawn_width(text: &str, size: f32, window: &Window, cx: &gpui::App) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    // One line, as the row draws it: a line break typed into a category's name is a space.
    let line: SharedString = text.replace(['\n', '\r'], " ").into();
    let mut style = window.text_style();
    style.font_family = gpui_component::ActiveTheme::theme(cx).font_family.clone();
    let run = style.to_run(line.len());
    let shaped = window
        .text_system()
        .shape_line(line, px(size), &[run], None);
    f32::from(shaped.width)
}

/// A place's picture beside its name, as Finder's and Mail's sidebars have one: each kind its
/// row's own (`kind_picture`), and a picture of what the others hold. All drawn in one colour,
/// the system's accent, as a source list's are (the maintainer, 2026-11-12: "monocolor").
fn picture(place: &Place) -> &'static str {
    match place {
        Place::All => "icons/place-all.svg",
        Place::Kind(kind) => crate::list::kind_picture(Some(*kind)).0,
        Place::Custom(_) => "icons/place-folder.svg",
        Place::Unfinished => "icons/place-unfinished.svg",
        Place::Finished => "icons/place-finished.svg",
        Place::Queue(_) => "icons/place-queue.svg",
    }
}

/// Every place the panel lists, in order, with its heading where a group
/// starts.
fn places(view: &Downloads) -> Vec<(Option<&'static str>, Place, SharedString)> {
    let settings = &view.drawn;
    let mut all = vec![(
        Some("All Downloads"),
        Place::All,
        SharedString::from(t("All")),
    )];
    for c in Category::ALL {
        let word = dm_shell::app::names::category_word(c);
        let label = name(&dm_desktop_base::format::titled(&word));
        all.push((None, Place::Kind(c), label.into()));
    }
    for custom in &settings.custom_categories {
        let custom = custom.name.trim();
        if !custom.is_empty() {
            all.push((
                None,
                Place::Custom(custom.to_string()),
                custom.to_string().into(),
            ));
        }
    }
    all.push((Some(""), Place::Unfinished, t("Unfinished").into()));
    all.push((None, Place::Finished, t("Finished").into()));
    // As the last read found them, not from every row at each frame.
    for (i, queue) in view.queue_list.iter().cloned().enumerate() {
        let heading = (i == 0).then_some("Queues");
        let label = name(&queue);
        all.push((heading, Place::Queue(queue), label.into()));
    }
    all
}

/// The side panel.
pub fn sidebar(
    view: &Downloads,
    theme: Theme,
    screen: &Screen,
    window: &mut Window,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let categories = view.drawn.categories();
    let wide = view.panel_width();
    div()
        .id("sidebar-panel")
        .relative()
        .w(px(wide))
        .flex_none()
        .h_full()
        .bg(crate::theme::frame(theme.sidebar))
        .border_r_1()
        .border_color(theme.separator)
        // Its edge dragged: the pointer's distance from the panel's left is its width, held
        // between the least and the most as it moves.
        .on_drag_move::<ResizingPanel>(cx.listener(
            |view, event: &gpui::DragMoveEvent<ResizingPanel>, _, cx| {
                let wide = f32::from(event.event.position.x - event.bounds.left());
                view.resize_panel(wide / crate::zoom::factor(), cx);
            },
        ))
        // Let go anywhere, over the panel or not: the last width kept.
        .on_mouse_up(
            gpui::MouseButton::Left,
            cx.listener(|view, _, _, cx| view.finish_panel_resize(cx)),
        )
        .on_mouse_up_out(
            gpui::MouseButton::Left,
            cx.listener(|view, _, _, cx| view.finish_panel_resize(cx)),
        )
        .child(
            div()
                .id("sidebar")
                .role(Role::List)
                .aria_label(t("Places"))
                .size_full()
                .overflow_y_scroll()
                .py_2()
                .flex()
                .flex_col()
                .children(places(view).into_iter().enumerate().map(
                    |(i, (heading, place, label))| {
                        place_row(
                            view,
                            theme,
                            screen,
                            window,
                            cx,
                            &categories,
                            i,
                            heading,
                            place,
                            label,
                        )
                    },
                )),
        )
        .child(
            div()
                .id("sidebar-edge")
                .absolute()
                .top_0()
                .right_0()
                .h_full()
                .w(px(EDGE))
                .cursor(gpui::CursorStyle::ResizeLeftRight)
                // A press here drags the edge and is not a place's click.
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                .on_drag(ResizingPanel, |dragged, _, _, cx| cx.new(|_| *dragged)),
        )
}

/// One place's row, under its group's heading where a group starts.
#[allow(clippy::too_many_arguments)]
fn place_row(
    view: &Downloads,
    theme: Theme,
    screen: &Screen,
    window: &Window,
    cx: &mut Context<Downloads>,
    categories: &dm_core::Categories,
    i: usize,
    heading: Option<&'static str>,
    place: Place,
    label: SharedString,
) -> impl IntoElement {
    let count = view.places.count(&place, &view.rows, categories);
    let chosen = place == view.place;
    let icon = picture(&place);
    let whole = cut(
        drawn_width(&label, crate::native::TEXT, window, cx),
        drawn_width(&count.to_string(), crate::native::SMALL_TEXT, window, cx),
        view.panel_width(),
        crate::zoom::factor(),
    )
    .then(|| label.clone());
    let heading = heading.map(|h| {
        div()
            // A source list's section heading: small,
            // in the secondary colour, a little bolder.
            .px_4()
            .pt_3()
            .pb_1()
            .text_size(px(crate::native::SMALL_TEXT))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(theme.muted)
            .when(!h.is_empty(), |d| d.child(screen.label(t(h))))
    });
    div().children(heading).child(
        div()
            .id(("place", i))
            .role(Role::ListItem)
            .aria_label(label.clone())
            .mx_2()
            .px_2()
            .h(px(26.0))
            .rounded(px(crate::native::RADIUS))
            .flex()
            .items_center()
            .justify_between()
            // Between a long name's ellipsis and the count; unseen while the name fits.
            .gap_2()
            .text_size(px(crate::native::TEXT))
            .when(chosen, |d| {
                d.bg(theme.selection).text_color(theme.on_selection)
            })
            .when(!chosen, |d| d.hover(|s| s.bg(theme.stripe)))
            // A name the panel cuts, read whole on hover.
            .when_some(whole, |d, whole| {
                d.tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(whole.clone()).build(window, cx)
                })
            })
            .on_click({
                let place = place.clone();
                cx.listener(move |view, _, _, cx| {
                    view.choose(place.clone());
                    cx.notify();
                })
            })
            // Its menu, as the egui window's (gaps row U12).
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |view, event: &gpui::MouseDownEvent, window, cx| {
                    view.open_place_menu(place.clone(), event.position);
                    // The list's keys, Escape among them, reach it.
                    window.focus(&view.focus, cx);
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(
                        gpui::svg()
                            .path(icon)
                            .size(px(16.0))
                            .flex_none()
                            // In the selection's own colour where chosen, as
                            // the words are.
                            .text_color(if chosen {
                                theme.on_selection
                            } else {
                                theme.accent
                            }),
                    )
                    // One line, cut with an ellipsis where the panel is too narrow for it.
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(screen.label(label)),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(crate::native::SMALL_TEXT))
                    .when(!chosen, |d| d.text_color(theme.muted))
                    .child(screen.label(count.to_string())),
            ),
    )
}

/// The panel's edge being dragged. Nothing is drawn under the pointer but the panel itself;
/// the left-right cursor stays on while it is, wherever the pointer goes.
#[derive(Clone, Copy)]
pub struct ResizingPanel;

impl Render for ResizingPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::canvas(
            |_, _, _| (),
            |_, _, window, _| window.set_window_cursor_style(gpui::CursorStyle::ResizeLeftRight),
        )
    }
}

impl Downloads {
    /// The side panel's width as drawn, in points at 100%: the one being dragged, else the one
    /// kept.
    pub(crate) fn panel_width(&self) -> f32 {
        SIDEBAR_WIDTH.held(self.panel_resizing.unwrap_or(self.drawn.sidebar_width))
    }

    /// The panel's edge dragged to `wide` points (at 100%): drawn at once, kept a few times a
    /// second meanwhile and when let go, as a column's edge is (`table.rs`).
    pub(crate) fn resize_panel(&mut self, wide: f32, cx: &mut Context<Self>) {
        let wide = SIDEBAR_WIDTH.held(wide);
        if self.panel_resizing == Some(wide) {
            return;
        }
        self.panel_resizing = Some(wide);
        let due = self
            .resize_kept
            .is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(200));
        if due {
            self.resize_kept = Some(std::time::Instant::now());
            self.keep_panel_width(wide);
        }
        cx.notify();
    }

    /// The drag let go: its last width kept, and drawn from the settings again.
    pub(crate) fn finish_panel_resize(&mut self, cx: &mut Context<Self>) {
        let Some(wide) = self.panel_resizing.take() else {
            return;
        };
        self.keep_panel_width(wide);
        self.resize_kept = None;
        self.read();
        cx.notify();
    }

    fn keep_panel_width(&self, wide: f32) {
        self.backend
            .keep_look(|settings| settings.sidebar_width = wide);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name is offered whole where its laid-out width passes the room the panel leaves it:
    /// the panel's width less the row's margins, picture and gaps, its edge line and the count's
    /// width; wider at a larger text size, and a wider count leaves less room.
    #[test]
    fn a_long_name_is_offered_whole_where_the_panel_cuts_it() {
        // At 200 points and 100%: 200 - 64 - 1 (the edge line) - 7 (a figure) = 128 to draw in.
        let count = 7.0;
        assert!(!cut(120.0, count, SIDEBAR_WIDTH.default, 1.0));
        assert!(!cut(126.9, count, SIDEBAR_WIDTH.default, 1.0));
        assert!(
            cut(127.5, count, SIDEBAR_WIDTH.default, 1.0),
            "within a pixel: offered"
        );
        assert!(cut(180.0, count, SIDEBAR_WIDTH.default, 1.0));
        // A name cut at the least width fits at the most.
        assert!(cut(150.0, count, SIDEBAR_WIDTH.least, 1.0));
        assert!(!cut(150.0, count, SIDEBAR_WIDTH.most, 1.0));
        // At 120% the margins and the name grow together; the same name, drawn 20% wider,
        // still fits.
        assert!(!cut(120.0 * 1.2, count * 1.2, SIDEBAR_WIDTH.default, 1.2));
        // A count of more figures leaves less room for the same name.
        assert!(!cut(115.0, count, SIDEBAR_WIDTH.default, 1.0));
        assert!(cut(115.0, 4.0 * count, SIDEBAR_WIDTH.default, 1.0));
    }

    /// The panel's bounds as the drag holds them: 150 to 400, whole points, today's 200 the
    /// default.
    #[test]
    fn the_panels_edge_is_held_between_its_least_and_most() {
        assert_eq!(SIDEBAR_WIDTH.default, 200.0, "the window as it always was");
        assert_eq!(SIDEBAR_WIDTH.held(0.0), 150.0);
        assert_eq!(SIDEBAR_WIDTH.held(-40.0), 150.0);
        assert_eq!(SIDEBAR_WIDTH.held(260.4), 260.0);
        assert_eq!(SIDEBAR_WIDTH.held(9_000.0), 400.0);
        assert_eq!(SIDEBAR_WIDTH.held(f32::NAN), 200.0);
    }
}
