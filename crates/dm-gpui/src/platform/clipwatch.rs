// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The clipboard watched (Options › General, "Offer to download links I
//! copy"): a link copied is offered above the list with Download and
//! Dismiss, several with Download… (the list of them to tick); with "Take a
//! copied link at once", taken without the offer. What a look finds, and
//! what is not offered again, is the application's
//! (`dm_shell::app::links::Clipboard`).
//!
//! On a phone (gaps row N5) the same watch, looked at only while dm's
//! window is in front: Android 10 and up let only the app in front read the
//! clipboard and answer the others as if it were empty, which would withdraw
//! an offer and make it again on coming back. A link copied in another app
//! is so offered as dm comes to the front.

use std::time::Duration;

use dm_desktop_base::i18n::{t, tf};
use dm_shell::app::links::Looked;
use gpui::{div, prelude::*, Context};

use crate::widgets::native;
use crate::window::list::Downloads;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// How often the clipboard is looked at while it is watched.
pub(crate) const EVERY: Duration = Duration::from_secs(1);

impl Downloads {
    /// Turns the watch on or off; turned on, it is seeded with what the
    /// clipboard holds now, so a link copied long ago is not offered.
    pub(crate) fn watch_clipboard(&mut self, on: bool) {
        // On a phone the window may open before Android gives it the focus,
        // and a read refused then is as an empty clipboard: seeded instead by
        // the first look, a moment later, in front.
        if cfg!(target_os = "android") {
            self.system.clipboard.turn_unread(on);
            return;
        }
        let platform = &self.backend.platform;
        self.system.clipboard.turn(on, || platform.clipboard());
    }

    /// Looks at the clipboard, where it is watched and a look is due: one
    /// link taken at once opens Download details, several their list.
    pub(crate) fn check_clipboard(&mut self, cx: &mut Context<Self>) {
        if !self.system.clipboard.on
            || self.system.clipboard_reading
            || !readable()
            || self
                .system
                .clipboard
                .polled
                .is_some_and(|at| at.elapsed() < EVERY)
        {
            return;
        }
        // Read off the window's thread, as a copied page can be megabytes
        // of text (gaps row 183), and looked at here once it is.
        self.system.clipboard_reading = true;
        let platform = self.backend.platform.clone();
        // A read the phone refused, dm's window gone behind as it read, is
        // no look.
        let read = cx.background_spawn(async move {
            let text = platform.clipboard();
            (text, readable())
        });
        cx.spawn(async move |this, cx| {
            let (text, read_in_front) = read.await;
            let _ = this.update(cx, |this, cx| {
                this.system.clipboard_reading = false;
                if this.system.clipboard.on && read_in_front {
                    this.clipboard_read(text, cx);
                }
            });
        })
        .detach();
    }

    /// What the clipboard held, looked at: one link taken at once opens
    /// Download details, several their list; else offered or passed over.
    fn clipboard_read(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        let start_copied = self.backend.settings().start_copied;
        match self.system.clipboard.look(text, &self.rows, start_copied) {
            Looked::Take(links) if links.len() == 1 => {
                crate::dialogs::details::open(cx.entity().downgrade(), links, cx);
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
        let url = self.system.clipboard.offered.clone()?;
        let several = self.system.clipboard.offered_all.len() > 1;
        let words = if several {
            tf(
                "Links on your clipboard: {}",
                &[&self.system.clipboard.offered_all.len()],
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
                // Where the link would get less than `LINK_AT_LEAST`, the buttons go under it
                // (a phone's width: on one line the link was cut to "…"); a desktop's window
                // keeps all on one line.
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(div().flex_none().child(screen.label(words)))
                // The link in what is left, cut short: a long one pushed
                // Download and Dismiss past the window's edge.
                .child(
                    div()
                        .flex_1()
                        .min_w(px(LINK_AT_LEAST))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(theme.muted)
                        .when(!several, |d| d.child(screen.label(shown))),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .ml_auto()
                        .gap(px(native::LABEL_GAP))
                        .child(
                            native::button(
                                "clipboard-download",
                                download.into(),
                                true,
                                theme,
                                screen,
                            )
                            .on_click(cx.listener(
                                move |view, _, _, cx| {
                                    let links = view.system.clipboard.offered_all.clone();
                                    view.system.clipboard.dismiss_all();
                                    if links.len() > 1 {
                                        // The list that holds them, each to tick.
                                        view.add_from_clipboard(cx);
                                    } else {
                                        crate::dialogs::details::open(
                                            cx.entity().downgrade(),
                                            vec![url.clone()],
                                            cx,
                                        );
                                    }
                                    cx.notify();
                                },
                            )),
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
                                view.system.clipboard.dismiss_all();
                                cx.notify();
                            })),
                        ),
                ),
        )
    }
}

/// The least width the offered link is given before Download and Dismiss go under it: 160
/// points less the room the bar's mark and "+N more" now take in the bar (gaps row E5), so a
/// phone's 412 points still keep the link on its words' line.
const LINK_AT_LEAST: f32 = 112.0;

/// Whether the clipboard can be read now: always on a desktop; on a phone
/// only while dm's window is in front.
fn readable() -> bool {
    #[cfg(target_os = "android")]
    {
        crate::platform::phone::clipboard_readable()
    }
    #[cfg(not(target_os = "android"))]
    {
        true
    }
}
