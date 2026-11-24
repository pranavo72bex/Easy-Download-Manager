// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Download links' sections, each a piece of the window that `render`
//! puts together in its order.

use super::*;

/// A listed link's words, worked out once for a frame: its place in the list, name, tick,
/// kind, size, folder and words on the page.
pub(super) type Row = (usize, String, bool, String, String, String, String);

impl Links {
    /// The types to list, Check all and Uncheck all, and what is left out.
    pub(super) fn filters(
        &self,
        theme: Theme,
        screen: &Screen,
        check: &impl Fn(&'static str, &'static str, bool) -> gpui::Stateful<gpui::Div>,
        body: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        body.child(
            div()
                .flex()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Only these types")))
                .child(div().w(px(200.0)).child(native::field(
                    &self.types,
                    "Only these types",
                    theme,
                )))
                .child(div().flex_1())
                .child(
                    native::button("check-all", t("Check all").into(), false, theme, screen)
                        .on_click(cx.listener(|this, _, _, cx| this.check_all(true, cx))),
                )
                .child(
                    native::button("uncheck-all", t("Uncheck all").into(), false, theme, screen)
                        .on_click(cx.listener(|this, _, _, cx| this.check_all(false, cx))),
                ),
        )
        // What is left out, as reference A offers it.
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(native::BUTTON_GAP))
                .child(
                    check("hide-html", "Hide HTML files", self.d.hide_html).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.d.hide_html = !this.d.hide_html;
                            cx.notify();
                        },
                    )),
                )
                .child(
                    check("hide-images", "Hide images", self.d.hide_images).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.d.hide_images = !this.d.hide_images;
                            cx.notify();
                        },
                    )),
                )
                .child(
                    check("hide-repeats", "Hide repeated files", self.d.hide_repeats).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.d.hide_repeats = !this.d.hide_repeats;
                            cx.notify();
                        }),
                    ),
                ),
        )
    }

    /// The links: a heading row, then only the rows in view.
    pub(super) fn link_list(
        &self,
        theme: Theme,
        screen: &Screen,
        rows: Vec<Row>,
        count: usize,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let column = |words: &'static str, width: Option<f32>| {
            let d = div()
                .px(px(space::XS))
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis();
            match width {
                Some(w) => d.w(px(w)).flex_none(),
                None => d.flex_1().min_w_0(),
            }
            .child(screen.label(t(words)))
        };
        div()
            .flex()
            .flex_col()
            .rounded(px(native::RADIUS))
            .border_1()
            .border_color(theme.separator)
            .bg(theme.background)
            .child(
                div()
                    .flex()
                    .h(px(ROW))
                    .items_center()
                    .px(px(space::XS))
                    .border_b_1()
                    .border_color(theme.separator)
                    .text_size(px(native::SMALL_TEXT))
                    .text_color(theme.muted)
                    .child(column("File name", None))
                    .child(column("Type", Some(80.0)))
                    .child(column("Size", Some(72.0)))
                    .child(column("Save to", Some(96.0)))
                    .child(column("Link text", Some(110.0)))
                    .child(div().w(px(52.0)).flex_none()),
            )
            .child(
                div().h(px(ROW * SHOWN.min(count.max(1)) as f32)).child(
                    uniform_list(
                        "link-rows",
                        count,
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            let screen = this.screen.clone();
                            range
                                .filter_map(|k| {
                                    let (i, name, on, kind, size, last, words) =
                                        rows.get(k)?.clone();
                                    let cell = |text: String, w: f32| {
                                        div()
                                            .w(px(w))
                                            .flex_none()
                                            .px(px(space::XS))
                                            .whitespace_nowrap()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .text_color(theme.muted)
                                            .child(screen.label(text))
                                    };
                                    Some(
                                        div()
                                            .w_full()
                                            .h(px(ROW))
                                            .flex()
                                            .items_center()
                                            .px(px(space::XS))
                                            .when(k % 2 == 1, |d| d.bg(theme.stripe))
                                            .child(
                                                div().flex_1().min_w_0().overflow_hidden().child(
                                                    native::checkbox(
                                                        ("link", i),
                                                        name.into(),
                                                        on,
                                                        theme,
                                                        &screen,
                                                    )
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        if let Some(l) = this.d.items.get_mut(i) {
                                                            l.1 = !l.1;
                                                            cx.notify();
                                                        }
                                                    })),
                                                ),
                                            )
                                            .child(cell(kind, 80.0))
                                            .child(cell(size, 72.0))
                                            .child(cell(last, 96.0))
                                            .child(cell(words, 110.0))
                                            .child(
                                                div()
                                                    .id(("edit", i))
                                                    .role(gpui::Role::Button)
                                                    .aria_label(t("Edit"))
                                                    .map(|c| {
                                                        crate::widgets::native::ringed(c, theme)
                                                    })
                                                    .w(px(52.0))
                                                    .flex_none()
                                                    .text_color(theme.accent_text)
                                                    .child(screen.label(t("Edit")))
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.edit(i, window, cx)
                                                        },
                                                    )),
                                            ),
                                    )
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.scroll)
                    .size_full(),
                ),
            )
    }

    /// As on a Mac: Add for later, to a queue, at the leading side; Cancel,
    /// then Start now (the default) at the end.
    pub(super) fn buttons(
        &self,
        theme: Theme,
        screen: &Screen,
        queues: Vec<SharedString>,
        queue_at: Option<usize>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        native::buttons(
            vec![
                native::button("later", t("Add for later").into(), false, theme, screen)
                    .on_click(cx.listener(|this, _, window, cx| this.add(true, window, cx)))
                    .into_any_element(),
                screen
                    .label(t("queue:to").trim_start_matches("queue:").to_string())
                    .into_any_element(),
                popup::button(
                    "links-queue",
                    dm_desktop_base::i18n::name(&self.d.queue).into(),
                    queues,
                    queue_at,
                    theme,
                    screen,
                )
                .into_any_element(),
            ],
            vec![
                native::button("cancel", t("Cancel").into(), false, theme, screen)
                    .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx)))
                    .into_any_element(),
                native::button("start", t("Start now").into(), true, theme, screen)
                    .on_click(cx.listener(|this, _, window, cx| this.add(false, window, cx)))
                    .into_any_element(),
            ],
        )
    }
}
