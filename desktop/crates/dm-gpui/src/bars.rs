// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The bars above the list that the egui window has, each with its words and
//! its ways on: a download paused by itself for want of room or of its
//! drive (Choose another folder…, Resume); downloads that did not finish
//! (Retry all, Hide, until another fails); and, until the browser's
//! extension has spoken to dm, the way to add it (each browser's page, Not
//! now).

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use dm_core::{Id, State};
use dm_desktop_base::i18n::{t, tf, tn};
use gpui::{div, prelude::*, Context};

use crate::list::Downloads;
use crate::native;
use crate::screen::Screen;
use crate::theme::Theme;
use crate::zoom::px;

/// Written once "Not now" is chosen on the add-to-browser bar.
const BROWSER_HINT_HIDDEN: &str = "browser-hint.hidden";

/// A bar's frame: a line above the list, its words first, its buttons after.
fn bar(theme: Theme) -> gpui::Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(native::LABEL_GAP))
        .px(px(12.0))
        .py(px(6.0))
        .border_b_1()
        .border_color(theme.separator)
}

/// What the bars remember: which failures were hidden, and whether the
/// browser's extension has been heard from (looked at every few seconds).
#[derive(Default)]
pub struct Bars {
    /// The failures hidden: the bar stays away until another fails.
    failed_hidden: BTreeSet<Id>,
    /// When the add-to-browser bar was last looked into, and whether it shows.
    browser: Option<(Instant, bool)>,
    /// When a look into it, off the window's thread, began; none while no
    /// look is under way.
    browser_looking: Option<Instant>,
}

impl Downloads {
    /// A download paused by itself for want of room or of its drive: what
    /// happened, and the two ways on — another folder (its unfinished parts
    /// moved there too), or Resume once there is room again.
    pub(crate) fn stalled_bar(
        &self,
        theme: Theme,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let row = self
            .rows
            .iter()
            .find(|r| r.state == State::Paused && !r.stalled.is_empty())?;
        let id = row.id;
        let words = tf(
            "{} is paused: {}.",
            &[
                &dm_shell::app::names::name_of(row),
                &dm_desktop_base::rows::stalled_said(row),
            ],
        );
        Some(
            bar(theme)
                .child(
                    div()
                        .flex_1()
                        .text_color(theme.warning)
                        .child(screen.label(words)),
                )
                .child(
                    native::button("stalled-resume", t("Resume").into(), false, theme, screen)
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.backend.resume(id);
                            view.read();
                            cx.notify();
                        })),
                )
                .child(
                    native::button(
                        "stalled-folder",
                        t("Choose another folder…").into(),
                        true,
                        theme,
                        screen,
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        crate::services::ask_then(
                            crate::services::Asked::folder(),
                            cx,
                            move |view: &mut Self, dir, _| match view.backend.relocate(id, dir) {
                                Ok(()) => {
                                    view.read();
                                }
                                Err(e) => view.say(e),
                            },
                        );
                    })),
                ),
        )
    }

    /// Downloads that failed, counted above the list with one button to try
    /// them all again, so a failure further down is not missed. Hidden, it
    /// stays away until another download fails.
    pub(crate) fn failed_bar(
        &self,
        theme: Theme,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let failed: BTreeSet<Id> = self
            .rows
            .iter()
            .filter(|r| matches!(r.state, State::Failed(_)))
            .map(|r| r.id)
            .collect();
        if failed.is_empty() || failed.is_subset(&self.bars.failed_hidden) {
            return None;
        }
        let count = failed.len();
        let retried = failed.clone();
        Some(
            bar(theme)
                .child(
                    div()
                        .flex_1()
                        .text_color(theme.warning)
                        .child(screen.label(tn(
                            "{} download didn't finish.||{} downloads didn't finish.",
                            count as u64,
                            &[&count],
                        ))),
                )
                .child(
                    native::button("failed-hide", t("Hide").into(), false, theme, screen).on_click(
                        cx.listener(move |view, _, _, cx| {
                            view.bars.failed_hidden.clone_from(&failed);
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    native::button("failed-retry", t("Retry all").into(), false, theme, screen)
                        .on_click(cx.listener(move |view, _, _, cx| {
                            for id in &retried {
                                view.backend.retry(*id);
                            }
                            view.read();
                            cx.notify();
                        })),
                ),
        )
    }

    /// Until the browser's extension has spoken to dm, the way to add it:
    /// each browser's page, or Not now, kept. Looked into every few seconds,
    /// not every frame.
    pub(crate) fn browser_bar(
        &mut self,
        theme: Theme,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let stale = self
            .bars
            .browser
            .is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(3));
        if stale && self.bars.browser_looking.is_none() {
            // Two files looked for off the window's thread, and drawn again
            // where the answer changed: a frame waited on the disk for them
            // (gaps row 180).
            let began = Instant::now();
            self.bars.browser_looking = Some(began);
            let home = dm_shell::settings::home();
            let looked = cx.background_spawn(async move {
                !dm_shell::host::browser_seen(&home) && !home.join(BROWSER_HINT_HIDDEN).exists()
            });
            cx.spawn(async move |this, cx| {
                let shows = looked.await;
                let _ = this.update(cx, |this, cx| {
                    this.bars.browser_looking = None;
                    // Not now, pressed while it looked, has the last word.
                    if this.bars.browser.is_some_and(|(at, _)| at > began) {
                        return;
                    }
                    let before = this.bars.browser.map(|(_, shows)| shows);
                    this.bars.browser = Some((Instant::now(), shows));
                    if before != Some(shows) {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        if !self.bars.browser.is_some_and(|(_, shows)| shows) {
            return None;
        }
        let pages = dm_desktop_base::project::extension_browsers()
            .into_iter()
            .enumerate()
            .map(|(n, (name, page, _))| {
                native::button(("add-to-browser", n), name.into(), false, theme, screen)
                    .on_click(cx.listener(move |view, _, _, _| view.open_path(page)))
            });
        Some(
            bar(theme)
                .child(
                    div()
                        .id("add-to-browser")
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(screen.label(t("Add dm to your browser:")))
                        .tooltip(|window, cx| {
                            gpui_component::tooltip::Tooltip::new(gpui::SharedString::from(t(
                                "Sends browser downloads to dm once the extension is added.",
                            )))
                            .build(window, cx)
                        }),
                )
                .children(pages)
                .child(div().flex_1())
                .child(
                    native::button("not-now", t("Not now").into(), false, theme, screen).on_click(
                        cx.listener(|view, _, _, cx| {
                            let home = dm_shell::settings::home();
                            let _ = std::fs::create_dir_all(&home);
                            let _ = std::fs::write(home.join(BROWSER_HINT_HIDDEN), b"");
                            view.bars.browser = Some((Instant::now(), false));
                            cx.notify();
                        }),
                    ),
                ),
        )
    }
}
