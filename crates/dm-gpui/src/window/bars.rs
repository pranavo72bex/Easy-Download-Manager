// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The bars above the list, each with its words and
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

use crate::widgets::native;
use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// Written once "Not now" is chosen on the add-to-browser bar.
const BROWSER_HINT_HIDDEN: &str = dm_core::paths::BROWSER_HINT_HIDDEN;

/// How long what was found about the browser's extension is believed before the add-to-browser
/// bar looks into it again.
const BROWSER_LOOKED_FOR: Duration = Duration::from_secs(3);

/// The least width of a bar's words beside its buttons.
const BAR_WORDS_MIN: f32 = 150.0;

/// The room a bar's sentence asks for before its buttons go under it (`words`).
const BAR_WORDS_BASIS: f32 = 300.0;

/// The browser bar's buttons at their words' width with the macOS and Linux least width,
/// not Windows' dialog width (`native::BUTTON_MIN`, 120): four of those left its words no
/// room in Windows' first window.
const BAR_BUTTON_MIN: f32 = if crate::platform::ON_A_PHONE {
    56.0
} else {
    80.0
};

/// A bar's body: its words first, its buttons after, wrapping where the window is narrow.
/// `framed` draws it as a bar: its mark, its edges.
pub(crate) fn body() -> gpui::Div {
    div()
        .flex()
        // Its words, then its buttons, from the right where the window is mirrored (E19).
        .map(crate::window::mirror::row)
        .flex_wrap()
        .items_center()
        .gap(px(native::LABEL_GAP))
}

/// A bar's sentence, beside its buttons (`actions`): it asks for `BAR_WORDS_BASIS` of the line
/// and takes what else is free; where its share and the buttons do not fit on one line (a
/// narrow window, a large text size), the buttons go under it together.
pub(crate) fn words(said: impl IntoElement) -> gpui::Div {
    div()
        .flex_grow(1.0)
        .flex_shrink(1.0)
        .flex_basis(px(BAR_WORDS_BASIS))
        .min_w(px(BAR_WORDS_MIN))
        .child(said)
}

/// A bar's buttons, kept together: they move to the next line as one, never one left beside
/// the words and the other under them (at 150% "Retry all" went under "1 download didn't
/// finish." with "Hide" still beside it).
pub(crate) fn actions() -> gpui::Div {
    div()
        .flex()
        .map(crate::window::mirror::row)
        .flex_none()
        .items_center()
        .gap(px(native::LABEL_GAP))
}

/// What a bar says, as its mark tells it at a glance: something went wrong, or something to
/// know or do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    /// A failure: a download that did not finish, one stalled, a problem from outside.
    Failure,
    /// An offer or a step to take: a link on the clipboard, an update, the browser's extension.
    Note,
}

impl Tone {
    /// The mark's letter, told by its shape and not its colour alone (`docs/ui-design.md`).
    fn letter(self) -> &'static str {
        match self {
            Tone::Failure => "!",
            Tone::Note => "i",
        }
    }
}

/// The bar's mark: a ring with "!" in the error colour for a failure, with "i" in the hint
/// colour for a note.
fn mark(tone: Tone, theme: Theme) -> gpui::Div {
    let colour = match tone {
        Tone::Failure => theme.warning,
        Tone::Note => theme.muted,
    };
    div()
        .flex_none()
        .size(px(16.0))
        .rounded_full()
        .border(px(1.5))
        .border_color(colour)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(crate::widgets::native::MARK_TEXT))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(colour)
        .child(tone.letter())
}

/// A bar above the list (gaps row E5): its mark, its body (the sentence in the text colour and
/// its buttons), and where `more` bars wait behind it, "+N more" at its end, which shows them
/// all. One bar where a message and its count were two.
pub(crate) fn framed(
    tone: Tone,
    body: gpui::Div,
    more: usize,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> gpui::AnyElement {
    div()
        .flex()
        // Its mark first, at the right where the window is mirrored, beside its sentence: it
        // stood at the far end, across the bar (gaps row E19).
        .map(crate::window::mirror::row)
        .items_center()
        .gap(px(native::LABEL_GAP))
        .px(px(space::L))
        .py(px(space::S))
        .border_b_1()
        .border_color(theme.separator)
        .child(mark(tone, theme))
        .child(div().flex_1().min_w_0().child(body))
        .when(more > 0, |d| {
            let words = tn("+{} more||+{} more", more as u64, &[&more]);
            d.child(
                div()
                    .id("more-bars")
                    // A button as native::button is: named, reached by Tab, pressed by Space or
                    // Return, its place ringed.
                    .role(gpui::Role::Button)
                    .aria_label(gpui::SharedString::from(words.clone()))
                    .tab_index(0)
                    .rounded(px(native::RADIUS))
                    .focus_visible(move |s| s.border_2().border_color(theme.ring(false)))
                    .flex_none()
                    .text_size(px(native::SMALL_TEXT))
                    .text_color(theme.accent_text)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.layout.bars.all = true;
                        cx.notify();
                    }))
                    .child(screen.label(words)),
            )
        })
        .into_any_element()
}

/// What the bars remember: which failures were hidden, and whether the
/// browser's extension has been heard from (looked at every few seconds).
#[derive(Default)]
pub struct Bars {
    /// "+N more" was clicked: every bar shown, until one or none is left (gaps row U26).
    pub(crate) all: bool,
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
            body().child(self::words(screen.label(words))).child(
                actions()
                    .child(
                        native::button("stalled-resume", t("Resume").into(), false, theme, screen)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.later_then_read(move |backend| backend.resume(id), cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        native::button(
                            "stalled-folder",
                            t("Choose another folder…").into(),
                            // Plain beside the toolbar's Add URL, the window's one filled button
                            // (`docs/ui-design.md` rule 1, gaps row E15).
                            false,
                            theme,
                            screen,
                        )
                        .on_click(cx.listener(move |_, _, _, cx| {
                            crate::platform::services::ask_then(
                                crate::platform::services::Asked::folder(),
                                cx,
                                move |view: &mut Self, dir, cx| view.relocate_later(id, dir, cx),
                            );
                        })),
                    ),
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
        if failed.is_empty() || failed.is_subset(&self.layout.bars.failed_hidden) {
            return None;
        }
        let count = failed.len();
        let retried = failed.clone();
        Some(
            body()
                .child(words(screen.label(tn(
                    "{} download didn't finish.||{} downloads didn't finish.",
                    count as u64,
                    &[&count],
                ))))
                .child(
                    actions()
                        .child(
                            native::button("failed-hide", t("Hide").into(), false, theme, screen)
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.layout.bars.failed_hidden.clone_from(&failed);
                                    cx.notify();
                                })),
                        )
                        .child(
                            native::button(
                                "failed-retry",
                                t("Retry all").into(),
                                false,
                                theme,
                                screen,
                            )
                            .on_click(cx.listener(
                                move |view, _, _, cx| {
                                    // Off the window's thread, as the toolbar's Resume is.
                                    let ids = retried.clone();
                                    view.later_then_read(
                                        move |backend| {
                                            for id in &ids {
                                                backend.retry(*id);
                                            }
                                        },
                                        cx,
                                    );
                                    cx.notify();
                                },
                            )),
                        ),
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
        // No browser extension reaches dm on a phone: a link comes by Share instead.
        if crate::platform::ON_A_PHONE {
            return None;
        }
        let stale = self
            .layout
            .bars
            .browser
            .is_none_or(|(at, _)| at.elapsed() > BROWSER_LOOKED_FOR);
        if stale && self.layout.bars.browser_looking.is_none() {
            // Two files looked for off the window's thread, and drawn again
            // where the answer changed: a frame waited on the disk for them
            // (gaps row 180).
            let began = Instant::now();
            self.layout.bars.browser_looking = Some(began);
            let home = dm_shell::settings::home();
            let looked = cx.background_spawn(async move {
                !dm_shell::host::browser_seen(&home) && !home.join(BROWSER_HINT_HIDDEN).exists()
            });
            cx.spawn(async move |this, cx| {
                let shows = looked.await;
                let _ = this.update(cx, |this, cx| {
                    this.layout.bars.browser_looking = None;
                    // Not now, pressed while it looked, has the last word.
                    if this.layout.bars.browser.is_some_and(|(at, _)| at > began) {
                        return;
                    }
                    let before = this.layout.bars.browser.map(|(_, shows)| shows);
                    this.layout.bars.browser = Some((Instant::now(), shows));
                    if before != Some(shows) {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        if !self.layout.bars.browser.is_some_and(|(_, shows)| shows) {
            return None;
        }
        let pages = dm_desktop_base::project::extension_browsers()
            .into_iter()
            .enumerate()
            .map(|(n, (name, page, _))| {
                native::button(("add-to-browser", n), name.into(), false, theme, screen)
                    .min_w(px(BAR_BUTTON_MIN))
                    .on_click(cx.listener(move |view, _, _, _| view.open_path(page)))
            });
        Some(
            // On one line where it fits (gaps row U26); where the window is narrower, the
            // browsers' buttons go on to a second line, "Not now" at its end, rather than being
            // cut off at the window's edge (gaps row V15).
            body()
                .child(
                    div()
                        .id("add-to-browser")
                        .flex_shrink(1.0)
                        // Room enough that, in a narrow window, the browsers' buttons go to the
                        // next line rather than squeeze beside it (gaps row V15).
                        .min_w(px(BAR_WORDS_MIN))
                        .overflow_hidden()
                        // Whole on a second line where the window is too narrow for one,
                        // as Windows' first window is (840 wide), never cut to "Add dm…".
                        .line_clamp(2)
                        .text_ellipsis()
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
                    native::button("not-now", t("Not now").into(), false, theme, screen)
                        .min_w(px(BAR_BUTTON_MIN))
                        .on_click(cx.listener(|view, _, _, cx| {
                            let home = dm_shell::settings::home();
                            let _ = std::fs::create_dir_all(&home);
                            let _ = std::fs::write(home.join(BROWSER_HINT_HIDDEN), b"");
                            view.layout.bars.browser = Some((Instant::now(), false));
                            cx.notify();
                        })),
                ),
        )
    }
}
