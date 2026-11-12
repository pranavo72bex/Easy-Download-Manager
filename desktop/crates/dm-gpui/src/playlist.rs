// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Playlist", a window of its own (B02's playlist): a YouTube playlist, or
//! another site's, read by dm itself as it opens; its videos listed with a
//! tick each (only the rows in view drawn), Select all, one Video quality,
//! and Start now or Add for later. What it adds is dm's
//! (`dm_desktop_base::file_info::playlist_adds`).

use dm_core::youtube::Playlist;
use dm_desktop_base::file_info;
use dm_desktop_base::i18n::{t, tf};
use gpui::{
    div, prelude::*, size, uniform_list, App, Bounds, Context, FocusHandle, KeyDownEvent,
    SharedString, UniformListScrollHandle, WeakEntity, Window, WindowBounds, WindowOptions,
};

use crate::a11y::Told;
use crate::list::Downloads;
use crate::native;
use crate::popup::{self, Pick};
use crate::screen::{Screen, Screens};
use crate::theme::Theme;
use crate::zoom::px;

/// A video's row in the list, in points.
const ROW: f32 = 22.0;
/// How many rows the list shows before it scrolls.
const SHOWN: usize = 10;

/// The Playlist window's view.
pub struct PlaylistWindow {
    view: WeakEntity<Downloads>,
    url: String,
    /// The playlist once read; `None` while it is.
    read: Option<Result<Playlist, String>>,
    /// Each video ticked or not.
    picked: Vec<bool>,
    /// The quality to take them at (`file_info::QUALITIES`' words).
    quality: String,
    says: Option<SharedString>,
    /// Answered: a second Return in the same moment adds nothing.
    done: bool,
    focus: FocusHandle,
    screen: Screen,
    scroll: UniformListScrollHandle,
}

impl PlaylistWindow {
    /// The playlist, as read.
    fn got(&mut self, read: Result<Playlist, String>, cx: &mut Context<Self>) {
        if let Ok(list) = &read {
            self.picked = vec![true; list.entries.len()];
        }
        self.read = Some(read);
        cx.notify();
    }

    /// Start now (`later` false) or Add for later: the ticked videos added.
    fn add(&mut self, later: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        let Some(Ok(list)) = &self.read else {
            return;
        };
        let (urls, with) =
            match file_info::playlist_adds(list, &self.picked, &self.quality, &self.url, later) {
                Ok(adds) => adds,
                Err(words) => {
                    self.says = Some(words.into());
                    cx.notify();
                    return;
                }
            };
        let count = urls.len();
        let added = self
            .view
            .update(cx, |view, cx| view.add(urls, with, cx))
            .unwrap_or_else(|_| Err(String::new()));
        match added {
            Ok(_) => {
                self.done = true;
                let _ = self.view.update(cx, |view, cx| {
                    view.say(dm_desktop_base::i18n::tn(
                        "Added {} download.||Added {} downloads.",
                        count as u64,
                        &[&count],
                    ));
                    cx.notify();
                });
                window.remove_window();
            }
            Err(e) => {
                self.says = Some(e.into());
                cx.notify();
            }
        }
    }

    /// Cancel, Escape or the close button: nothing added.
    fn cancel(&mut self, window: &mut Window) {
        self.done = true;
        window.remove_window();
    }

    /// A choice from the quality pop-up.
    fn pick(&mut self, pick: &Pick, cx: &mut Context<Self>) {
        if pick.field == "quality" {
            if let Some((value, _)) = file_info::QUALITIES.get(pick.index) {
                self.quality = value.to_string();
                cx.notify();
            }
        }
    }
}

impl Render for PlaylistWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let ready = matches!(&self.read, Some(Ok(_)));
        let all = !self.picked.is_empty() && self.picked.iter().all(|p| *p);
        let quality_at = file_info::QUALITIES
            .iter()
            .position(|(v, _)| *v == self.quality)
            .unwrap_or(0);
        let qualities: Vec<SharedString> = file_info::QUALITIES
            .iter()
            .map(|(_, words)| SharedString::from(t(words)))
            .collect();
        let body = match &self.read {
            None => div()
                .text_color(theme.muted)
                .child(screen.label(t("Reading the playlist…")))
                .into_any_element(),
            Some(Err(why)) => native::note(
                tf("The playlist could not be read: {}", &[why]),
                theme.warning,
                &screen,
            )
            .into_any_element(),
            Some(Ok(list)) => {
                let entries = list.entries.clone();
                let count = entries.len();
                div()
                        .flex()
                        .flex_col()
                        .gap(px(native::ROW_GAP))
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(screen.label(list.title.clone())),
                        )
                        .child(native::note(
                            file_info::playlist_count_words(list),
                            theme.muted,
                            &screen,
                        ))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(native::BUTTON_GAP))
                                .child(
                                    native::checkbox(
                                        "all",
                                        t("Select all").into(),
                                        all,
                                        theme,
                                        &screen,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.picked.fill(!all);
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(div().flex_1())
                                .child(screen.label(t("Video quality")))
                                .child(popup::button(
                                    "quality",
                                    qualities[quality_at].clone(),
                                    qualities.clone(),
                                    Some(quality_at),
                                    theme,
                                    &screen,
                                )),
                        )
                        // Each video with its number and length, ticked to be
                        // taken; only the rows in view are drawn.
                        .child(
                            div()
                                .h(px(ROW * SHOWN.min(count.max(1)) as f32 + 8.0))
                                .rounded(px(native::RADIUS))
                                .border_1()
                                .border_color(theme.separator)
                                .bg(theme.background)
                                .p(px(4.0))
                                .child(
                                    uniform_list(
                                        "videos",
                                        count,
                                        cx.processor(
                                            move |this, range: std::ops::Range<usize>, _, cx| {
                                                let screen = this.screen.clone();
                                                range
                                                    .filter_map(|i| {
                                                        let entry = entries.get(i)?;
                                                        let ticked = this
                                                            .picked
                                                            .get(i)
                                                            .copied()
                                                            .unwrap_or(false);
                                                        Some(
                                                    div().w_full().h(px(ROW)).flex().items_center().child(
                                                        native::checkbox(
                                                            ("video", i),
                                                            file_info::playlist_entry_words(
                                                                i, entry,
                                                            )
                                                            .into(),
                                                            ticked,
                                                            theme,
                                                            &screen,
                                                        )
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                if let Some(p) =
                                                                    this.picked.get_mut(i)
                                                                {
                                                                    *p = !*p;
                                                                    cx.notify();
                                                                }
                                                            },
                                                        )),
                                                    ),
                                                )
                                                    })
                                                    .collect::<Vec<_>>()
                                            },
                                        ),
                                    )
                                    .track_scroll(&self.scroll)
                                    .size_full(),
                                ),
                        )
                        .into_any_element()
            }
        };
        let button = |id: &'static str, words: &'static str, default: bool| {
            let greyed = !ready && id != "cancel";
            native::button(id, t(words).into(), default, theme, &screen)
                .when(greyed, |b| b.opacity(0.5))
                // Told off to a screen reader too (gaps row U23).
                .off_if(greyed)
        };
        crate::native::dialog("playlist", t("Playlist"), theme, cx)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, pick: &Pick, _, cx| this.pick(pick, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter" {
                    this.add(false, window, cx);
                }
            }))
            .child(crate::fit::fitted(
                native::body(theme)
                    .child(
                        div()
                            .text_color(theme.muted)
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(screen.label(self.url.clone())),
                    )
                    .child(body)
                    .when_some(self.says.clone(), |d, says| {
                        d.child(native::note(says, theme.warning, &screen))
                    })
                    // As on a Mac: Add for later at the leading side, then
                    // Cancel, and Start now (the default) at the end.
                    .child(native::buttons(
                        vec![button("later", "Add for later", false)
                            .on_click(cx.listener(|this, _, window, cx| this.add(true, window, cx)))
                            .into_any_element()],
                        vec![
                            button("cancel", "Cancel", false)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            button("start", "Start now", true)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.add(false, window, cx)),
                                )
                                .into_any_element(),
                        ],
                    )),
            ))
    }
}

/// Opens "Playlist" for `url`, read by dm as it opens, or already read
/// (another site's page Download details found lists several videos), its
/// videos to be taken at `quality`.
pub fn open(view: WeakEntity<Downloads>, url: String, read: Option<Playlist>, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, url, read, cx));
}

fn open_now(view: WeakEntity<Downloads>, url: String, read: Option<Playlist>, cx: &mut App) {
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let backend = downloads.read(cx).backend.clone();
    let quality = backend.settings().stream_quality.clone();
    let bounds = Bounds::centered(None, size(px(560.0), px(420.0)), cx);
    let opened = crate::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Playlist").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let known = read.is_some();
            let playlist = cx.new(|cx| {
                let focus = cx.focus_handle();
                let mut this = PlaylistWindow {
                    view,
                    url: url.clone(),
                    read: None,
                    picked: Vec::new(),
                    quality,
                    says: None,
                    done: false,
                    focus,
                    screen,
                    scroll: UniformListScrollHandle::new(),
                };
                if let Some(list) = read {
                    this.got(Ok(list), cx);
                }
                this
            });
            playlist.update(cx, |p, cx| window.focus(&p.focus, cx));
            if !known {
                // Read by the application (`lookups::read_playlist_later`),
                // told back on this window's thread.
                let (tell, told) = async_channel::bounded(1);
                dm_shell::app::lookups::read_playlist_later(&backend, url, move |got| {
                    let _ = tell.try_send(got);
                });
                let weak = playlist.downgrade();
                window
                    .spawn(cx, async move |cx| {
                        if let Ok(got) = told.recv().await {
                            let _ = weak.update(cx, |p, cx| p.got(got, cx));
                        }
                    })
                    .detach();
            }
            crate::native::close_is_cancel(&playlist, window, cx);
            cx.new(|cx| gpui_component::Root::new(playlist, window, cx))
        },
    );
    if let Err(e) = opened {
        eprintln!("dm-gpui: Playlist could not be opened: {e}");
    }
}

impl crate::native::CloseButton for PlaylistWindow {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
