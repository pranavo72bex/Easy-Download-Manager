// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The clipboard watched, as the egui window watches it (Options › General,
//! "Offer to download links I copy"): a link copied is offered above the
//! list with Download and Dismiss, several with Download… (the list of
//! them to tick); with "Take a copied link at once", taken without the
//! offer. What a look finds, and what is not offered again, is the
//! application's (`dm_shell::app::links::Clipboard`).

use std::time::Duration;

use dm_desktop_base::i18n::{t, tf};
use dm_shell::app::links::Looked;
use gpui::{div, prelude::*, px, Context};

use crate::list::Downloads;
use crate::native;
use crate::screen::Screen;
use crate::theme::Theme;

/// How often the clipboard is looked at while it is watched.
pub(crate) const EVERY: Duration = Duration::from_secs(1);

impl Downloads {
    /// Turns the watch on or off; turned on, it is seeded with what the
    /// clipboard holds now, so a link copied long ago is not offered.
    pub(crate) fn watch_clipboard(&mut self, on: bool) {
        let platform = &self.backend.platform;
        self.clipboard.turn(on, || platform.clipboard());
    }

    /// Looks at the clipboard, where it is watched and a look is due: one
    /// link taken at once opens Download details, several their list.
    pub(crate) fn check_clipboard(&mut self, cx: &mut Context<Self>) {
        if !self.clipboard.on || self.clipboard.polled.is_some_and(|at| at.elapsed() < EVERY) {
            return;
        }
        let text = self.backend.platform.clipboard();
        let start_copied = self.backend.settings().start_copied;
        match self.clipboard.look(text, &self.rows, start_copied) {
            Looked::Take(links) if links.len() == 1 => {
                crate::details::open(cx.entity().downgrade(), links, cx);
            }
            Looked::Take(_) => self.add_from_clipboard(cx),
            Looked::Offered => cx.notify(),
            Looked::Nothing => {}
        }
    }

    /// The offer above the list: the link copied with Download and Dismiss,
    /// or how many with Download… and Dismiss.
    pub(crate) fn clipboard_bar(
        &self,
        theme: Theme,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let url = self.clipboard.offered.clone()?;
        let several = self.clipboard.offered_all.len() > 1;
        let words = if several {
            tf(
                "Links on your clipboard: {}",
                &[&self.clipboard.offered_all.len()],
            )
        } else {
            t("Link on your clipboard:").to_string()
        };
        let shown = dm_desktop_base::format::shown_url(&url);
        let download = if several {
            format!("{}…", t("Download"))
        } else {
            t("Download").to_string()
        };
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .px(px(12.0))
                .py(px(6.0))
                .border_b_1()
                .border_color(theme.separator)
                .child(div().flex_none().child(screen.label(words)))
                // The link in what is left, cut short: a long one pushed
                // Download and Dismiss past the window's edge.
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(theme.muted)
                        .when(!several, |d| d.child(screen.label(shown))),
                )
                .child(
                    native::button("clipboard-download", download.into(), true, theme, screen)
                        .on_click(cx.listener(move |view, _, _, cx| {
                            let links = view.clipboard.offered_all.clone();
                            view.clipboard.dismiss_all();
                            if links.len() > 1 {
                                // The list that holds them, each to tick.
                                view.add_from_clipboard(cx);
                            } else {
                                crate::details::open(
                                    cx.entity().downgrade(),
                                    vec![url.clone()],
                                    cx,
                                );
                            }
                            cx.notify();
                        })),
                )
                .child(
                    native::button(
                        "clipboard-dismiss",
                        t("Dismiss").into(),
                        false,
                        theme,
                        screen,
                    )
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.clipboard.dismiss_all();
                        cx.notify();
                    })),
                ),
        )
    }
}
