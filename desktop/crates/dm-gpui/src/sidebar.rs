// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The side panel, as a macOS source list (Finder's, Mail's): a heading for
//! each group, each place with how many it holds, the chosen one drawn in
//! the system's selection colour. The places, their counts and their order
//! are the egui window's (`dm_shell::app::places`): All Downloads (All,
//! each kind, a person's own categories), Unfinished, Finished, and Queues.

use dm_core::Category;
use dm_desktop_base::i18n::{name, t};
use dm_shell::app::places::Place;
use gpui::{div, prelude::*, Context, Role, SharedString, Window};

use crate::list::Downloads;
use crate::screen::Screen;
use crate::theme::Theme;
use crate::zoom::px;

/// The side panel's width, in points.
pub const WIDTH: f32 = 200.0;

/// A place's picture and its colour beside its name, as Finder's and Mail's sidebars have one:
/// each kind its row's own (`kind_picture`), and a picture of what the others hold.
fn picture(place: &Place) -> (&'static str, gpui::Rgba) {
    match place {
        Place::All => ("icons/place-all.svg", gpui::rgb(0x2b6cc4)),
        Place::Kind(kind) => crate::list::kind_picture(Some(*kind)),
        Place::Custom(_) => ("icons/place-folder.svg", gpui::rgb(0x5b8bd0)),
        Place::Unfinished => ("icons/place-unfinished.svg", gpui::rgb(0xd9822b)),
        Place::Finished => ("icons/place-finished.svg", gpui::rgb(0x3d8b37)),
        Place::Queue(_) => ("icons/place-queue.svg", gpui::rgb(0x7f8c8d)),
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
    _window: &mut Window,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let categories = view.drawn.categories();
    div()
        .id("sidebar")
        .role(Role::List)
        .aria_label("Places")
        .w(px(WIDTH))
        .flex_none()
        .h_full()
        .overflow_y_scroll()
        .bg(theme.sidebar)
        .border_r_1()
        .border_color(theme.separator)
        .py_2()
        .flex()
        .flex_col()
        .children(
            places(view)
                .into_iter()
                .enumerate()
                .map(|(i, (heading, place, label))| {
                    let count = view.places.count(&place, &view.rows, &categories);
                    let chosen = place == view.place;
                    let (icon, colour) = picture(&place);
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
                            .text_size(px(crate::native::TEXT))
                            .when(chosen, |d| {
                                d.bg(theme.selection).text_color(theme.on_selection)
                            })
                            .when(!chosen, |d| d.hover(|s| s.bg(theme.stripe)))
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.choose(place.clone());
                                cx.notify();
                            }))
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
                                                colour
                                            }),
                                    )
                                    .child(screen.label(label)),
                            )
                            .child(
                                div()
                                    .text_size(px(crate::native::SMALL_TEXT))
                                    .when(!chosen, |d| d.text_color(theme.muted))
                                    .child(screen.label(count.to_string())),
                            ),
                    )
                }),
        )
}
