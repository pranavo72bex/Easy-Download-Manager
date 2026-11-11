// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Download complete" (B63), one window for everything that finished,
//! newest first, each file with its own Open, Open with… and Open folder;
//! Close, and "Don't show this window again", kept once it closes.

use dm_core::Id;
use dm_desktop_base::format;
use dm_desktop_base::i18n::{t, tf};
use gpui::{
    div, prelude::*, size, App, Bounds, Context, WeakEntity, Window, WindowBounds, WindowOptions,
};

use crate::list::Downloads;
use crate::native;
use crate::screen::{Screen, Screens};
use crate::theme::Theme;
use crate::zoom::px;

/// The window's view.
pub struct Complete {
    view: WeakEntity<Downloads>,
    /// "Don't show this window again" is ticked.
    never: bool,
    screen: Screen,
    /// The window's keyboard: nothing in it takes typing, and Escape and
    /// Return reach it only through a focus of its own.
    focus: gpui::FocusHandle,
    /// What it lists, as last drawn: drawn again only when that changes.
    listed: Vec<Finished>,
}

/// One finished download as the window lists it: all it draws of one.
#[derive(Clone, PartialEq)]
struct Finished {
    id: Id,
    name: String,
    size: u64,
    path: String,
    at: Option<u64>,
}

/// What the window lists of `view`, the newest first, where the eye lands.
fn finished(view: &Downloads) -> Vec<Finished> {
    let mut listed: Vec<Finished> = view
        .complete
        .iter()
        .filter_map(|id| view.rows.iter().find(|r| r.id == *id))
        .map(|row| Finished {
            id: row.id,
            name: dm_shell::app::names::name_of(row),
            size: row.progress.total.unwrap_or(row.progress.downloaded),
            path: row.path.clone(),
            at: row.finished_at,
        })
        .collect();
    listed.sort_by_key(|f| std::cmp::Reverse(f.at));
    listed
}

impl Complete {
    /// Closes the window, keeping a ticked "Don't show this window again".
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let never = self.never;
        let _ = self
            .view
            .update(cx, |view, cx| view.complete_closed(never, cx));
        window.remove_window();
    }

    /// Open, Open with… or Open folder for one file; it leaves the list.
    /// Open and Open with… run the file: a person's click does, a script's
    /// a moment ago does not, and the file stays listed for the person.
    fn act(&mut self, id: Id, path: String, how: u8, cx: &mut Context<Self>) {
        let _ = self.view.update(cx, |view, cx| {
            if how < 2 {
                if let Some(why) = view.scripted.run_refused() {
                    view.say(why.to_string());
                    cx.notify();
                    return;
                }
            }
            match how {
                0 => view.open_path(&path),
                1 => view.open_path_with(&path),
                _ => view.reveal_path(&path),
            }
            view.complete_done(id, cx);
        });
    }
}

impl Render for Complete {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let Some(view) = self.view.upgrade() else {
            return div().into_any_element();
        };
        let rows = finished(view.read(cx));
        self.listed = rows.clone();
        let several = rows.len() > 1;
        let never = self.never;
        crate::native::dialog("complete", t("Download complete"), theme, cx)
            .track_focus(&self.focus)
            // Return presses Close, the default, as Escape does: the window
            // opens of its own accord, over whatever has the keys, and a
            // Return meant for another dialog must not run the file that just
            // finished. Running it takes a click on Open.
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter" {
                    this.close(window, cx);
                }
            }))
            .child(crate::fit::fitted(
                native::body(theme)
                    .when(several, |d| {
                        d.child(screen.label(tf("{} downloads finished.", &[&rows.len()])))
                    })
                    .child(
                        div()
                            .id("finished")
                            .max_h(px(360.0))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(12.0))
                            .children(rows.iter().enumerate().map(|(i, row)| {
                                let name = row.name.clone();
                                let size = format::bytes(row.size);
                                let (id, open_path, with_path, folder_path) =
                                    (row.id, row.path.clone(), row.path.clone(), row.path.clone());
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .whitespace_nowrap()
                                                    .overflow_hidden()
                                                    .text_ellipsis()
                                                    .child(screen.label(name.clone())),
                                            )
                                            .child(
                                                div()
                                                    .text_color(theme.muted)
                                                    .child(screen.label(size)),
                                            ),
                                    )
                                    .child(native::note(row.path.clone(), theme.muted, &screen))
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(native::LABEL_GAP))
                                            .child(
                                                native::button(
                                                    ("open", i),
                                                    t("Open").into(),
                                                    false,
                                                    theme,
                                                    &screen,
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.act(id, open_path.clone(), 0, cx)
                                                })),
                                            )
                                            .child(
                                                native::button(
                                                    ("open-with", i),
                                                    t("Open with…").into(),
                                                    false,
                                                    theme,
                                                    &screen,
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.act(id, with_path.clone(), 1, cx)
                                                })),
                                            )
                                            .child(
                                                native::button(
                                                    ("folder", i),
                                                    t("Open folder").into(),
                                                    false,
                                                    theme,
                                                    &screen,
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.act(id, folder_path.clone(), 2, cx)
                                                })),
                                            ),
                                    )
                            })),
                    )
                    .child(native::buttons(
                        vec![native::checkbox(
                            "never",
                            t("Don't show this window again").into(),
                            never,
                            theme,
                            &screen,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.never = !this.never;
                            cx.notify();
                        }))
                        .into_any_element()],
                        vec![
                            native::button("close", t("Close").into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)))
                                .into_any_element(),
                        ],
                    )),
            ))
            .into_any_element()
    }
}

/// Opens "Download complete", in the corner above the status bar.
pub fn open(view: WeakEntity<Downloads>, cx: &mut App) -> Option<gpui::AnyWindowHandle> {
    let downloads = view.upgrade()?;
    let bounds = Bounds::centered(None, size(px(440.0), px(220.0)), cx);
    let opened = crate::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Download complete").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let complete = cx.new(|cx| {
                // Drawn again as more finish, and only then: drawn each time
                // the list was, it was drawn with every download's progress.
                cx.observe(&downloads, |this: &mut Complete, downloads, cx| {
                    if finished(downloads.read(cx)) != this.listed {
                        cx.notify();
                    }
                })
                .detach();
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                Complete {
                    view,
                    never: false,
                    screen,
                    focus,
                    listed: Vec::new(),
                }
            });
            crate::native::close_is_cancel(&complete, window, cx);
            cx.new(|cx| gpui_component::Root::new(complete, window, cx))
        },
    );
    opened.ok().map(Into::into)
}

impl crate::native::CloseButton for Complete {
    fn close_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
    }
}
