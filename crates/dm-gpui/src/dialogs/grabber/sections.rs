// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Grabber's sections, each a piece of the window that `render` puts
//! together in its order.

use super::*;

impl Grabber {
    /// What the last Find files found.
    pub(super) fn state(
        &self,
        theme: Theme,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        match &*dm_core::lock(&self.d.state) {
            Grab::Idle => None,
            // How far it has come, and a way to stop it keeping what it found (gaps row U25).
            Grab::Looking => {
                use std::sync::atomic::Ordering;
                let pages = self.watch.pages.load(Ordering::Relaxed);
                let files = self.watch.files.load(Ordering::Relaxed);
                let stopping = self.watch.stop.load(Ordering::Relaxed);
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(native::LABEL_GAP))
                        .child(div().flex_1().text_color(theme.muted).child(screen.label(
                            if pages == 0 {
                                t("Reading the page…").to_string()
                            } else {
                                tf("Reading… {} pages, {} files found", &[&pages, &files])
                            },
                        )))
                        .when(pages > 0, |d| {
                            d.child(
                                native::button(
                                    "stop-reading",
                                    t("Stop").into(),
                                    false,
                                    theme,
                                    screen,
                                )
                                .when(stopping, |b| b.opacity(0.5))
                                .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                            )
                        })
                        .into_any_element(),
                )
            }
            Grab::Failed(e) => {
                Some(native::note(e.clone(), theme.warning, screen).into_any_element())
            }
            Grab::Found(links, _, _) if links.is_empty() => Some(
                div()
                    .child(screen.label(t("No files of those types are linked from that page.")))
                    .into_any_element(),
            ),
            Grab::Found(links, pages, stopped) => {
                Some(self.found(links, pages, stopped, theme, screen, cx))
            }
        }
    }

    /// The files found, each with its tick, under Check all and Uncheck all.
    fn found(
        &self,
        links: &[(String, bool)],
        pages: &usize,
        stopped: &bool,
        theme: Theme,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let count = links.len();
        let rows: Vec<(String, bool)> = links
            .iter()
            .map(|(link, on)| {
                let shown = format::shown_url(link);
                let words = if self.listed.contains(&dm_shell::lists::bare(link)) {
                    tf("{} (already in the list)", &[&shown])
                } else {
                    shown
                };
                (words, *on)
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(native::ROW_GAP))
            // Counted with a colon, which needs no plural.
            .child(screen.label(tf(
                "Files found: {} · pages read: {}",
                &[&count, pages],
            )))
            .when(*stopped, |d| {
                d.child(native::note(
                    t("It ran out of time before reading every page. These are the files it found."),
                    theme.muted,
                    screen,
                ))
            })
            .child(
                div()
                    .flex()
                    .gap(px(native::BUTTON_GAP))
                    .child(
                        native::button("check-all", t("Check all").into(), false, theme, screen)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Grab::Found(links, _, _) = &mut *dm_core::lock(&this.d.state) {
                                    for l in links.iter_mut() {
                                        l.1 = true;
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        native::button("uncheck-all", t("Uncheck all").into(), false, theme, screen)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Grab::Found(links, _, _) = &mut *dm_core::lock(&this.d.state) {
                                    for l in links.iter_mut() {
                                        l.1 = false;
                                    }
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .h(px(ROW * SHOWN.min(count) as f32 + 8.0))
                    .rounded(px(native::RADIUS))
                    .border_1()
                    .border_color(theme.separator)
                    .bg(theme.background)
                    .p(px(space::XS))
                    .child(
                        uniform_list(
                            "found",
                            count,
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                let screen = this.screen.clone();
                                range
                                    .filter_map(|i| {
                                        let (words, on) = rows.get(i)?.clone();
                                        Some(
                                            div().w_full().h(px(ROW)).flex().items_center().child(
                                                native::checkbox(("found", i), words.into(), on, theme, &screen)
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        if let Grab::Found(links, _, _) =
                                                            &mut *dm_core::lock(&this.d.state)
                                                        {
                                                            if let Some(l) = links.get_mut(i) {
                                                                l.1 = !l.1;
                                                            }
                                                        }
                                                        cx.notify();
                                                    })),
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
            .into_any_element()
    }

    /// Saving, and the saved searches, under the rest of the form.
    pub(super) fn saving(
        &self,
        theme: Theme,
        screen: &Screen,
        saved: &[dm_shell::lists::Search],
        body: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        body.child(
            div()
                .flex()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Save this search as")))
                .child(div().w(px(180.0)).child(native::field(
                    &self.name,
                    "Save this search as",
                    theme,
                )))
                .child(screen.label(t("Run every (hours, 0 for manual)")))
                .child(div().w(px(60.0)).child(native::field(
                    &self.every,
                    "Run every (hours, 0 for manual)",
                    theme,
                )))
                .child(
                    native::button("save", t("Save search").into(), false, theme, screen)
                        .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                ),
        )
        .when(!self.d.says.is_empty(), |d| {
            d.child(native::note(self.d.says.clone(), theme.warning, screen))
        })
        .when(!saved.is_empty(), |d| {
            d.child(screen.label(t("Saved searches")))
                .children(saved.iter().enumerate().map(|(i, search)| {
                    let (run, delete) = (search.name.clone(), search.name.clone());
                    div()
                        .flex()
                        .items_center()
                        .gap(px(native::LABEL_GAP))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(screen.label(format!("{} — {}", search.name, search.page))),
                        )
                        .child(
                            native::button(("run", i), t("Run").into(), false, theme, screen)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.saved_search(run.clone(), true, cx)
                                })),
                        )
                        .child(
                            native::button(
                                ("delete", i),
                                t("Delete search").into(),
                                false,
                                theme,
                                screen,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.saved_search(delete.clone(), false, cx),
                            )),
                        )
                }))
        })
    }

    /// Close, and Add for later and Start now once there are files to add.
    pub(super) fn buttons(
        theme: Theme,
        screen: &Screen,
        found_any: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        native::buttons(
            Vec::new(),
            std::iter::once(
                native::button("close", t("Close").into(), !found_any, theme, screen)
                    .on_click(cx.listener(|_, _, window, _| window.remove_window()))
                    .into_any_element(),
            )
            .chain(found_any.then(|| {
                native::button("add-later", t("Add for later").into(), false, theme, screen)
                    .on_click(cx.listener(|this, _, window, cx| this.add(true, window, cx)))
                    .into_any_element()
            }))
            // Started at once, where Add for later needed Start Queue after it (gaps
            // row U25).
            .chain(found_any.then(|| {
                native::button("start-now", t("Start now").into(), true, theme, screen)
                    .on_click(cx.listener(|this, _, window, cx| this.add(false, window, cx)))
                    .into_any_element()
            }))
            .collect(),
        )
    }

    /// The ticks for following links to other sites and keeping the site's
    /// folders, under the page depth.
    pub(super) fn follow(
        other: bool,
        keep: bool,
        theme: Theme,
        screen: &Screen,
        width: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        native::row(
            None,
            width,
            screen,
            div()
                .flex()
                .flex_col()
                .gap(px(native::ROW_GAP))
                .child(
                    native::checkbox(
                        "other-sites",
                        t("Also follow links to other sites").into(),
                        other,
                        theme,
                        screen,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.d.other_sites = !this.d.other_sites;
                        cx.notify();
                    })),
                )
                .child(
                    native::checkbox(
                        "keep-layout",
                        t("Keep the site's folders").into(),
                        keep,
                        theme,
                        screen,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.d.keep_layout = !this.d.keep_layout;
                        cx.notify();
                    })),
                ),
        )
    }
}
