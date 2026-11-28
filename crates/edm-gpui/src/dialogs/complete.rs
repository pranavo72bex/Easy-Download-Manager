// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Download complete" (B63), one window for everything that finished,
//! newest first, each file with its own Open and Open folder (Open with… is on its row's menu);
//! Close, and "Don’t show this window again", kept once it closes.

use edm_core::Id;
use edm_desktop_base::format;
use edm_desktop_base::i18n::{t, tf};
use gpui::{
    div, prelude::*, size, App, Bounds, Context, WeakEntity, Window, WindowBounds, WindowOptions,
};

use crate::widgets::native;
use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// The room a file's name and place ask for before its buttons go under them.
const NAME_BASIS: f32 = 240.0;

/// The window's view.
pub struct Complete {
    view: WeakEntity<Downloads>,
    /// "Don’t show this window again" is ticked.
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
        .windows
        .complete
        .iter()
        .filter_map(|id| view.rows.iter().find(|r| r.id == *id))
        .map(|row| Finished {
            id: row.id,
            name: edm_shell::app::names::name_of(row),
            size: row.progress.total.unwrap_or(row.progress.downloaded),
            path: row.path.clone(),
            at: row.finished_at,
        })
        .collect();
    listed.sort_by_key(|f| std::cmp::Reverse(f.at));
    listed
}

impl Complete {
    /// Closes the window, keeping a ticked "Don’t show this window again".
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let never = self.never;
        let _ = self
            .view
            .update(cx, |view, cx| view.complete_closed(never, cx));
        window.remove_window();
    }

    /// Open (`run`) or Open folder for one file; it leaves the list. Open runs the file: a
    /// person's click does, a script's a moment ago does not, and the file stays listed for the
    /// person.
    fn act(&mut self, id: Id, path: String, run: bool, cx: &mut Context<Self>) {
        let _ = self.view.update(cx, |view, cx| {
            if run {
                if let Some(why) = view.port.scripted.run_refused() {
                    view.say(why.to_string());
                    cx.notify();
                    return;
                }
                view.open_path(&path);
            } else {
                view.reveal_path(&path);
            }
            view.complete_done(id, cx);
        });
    }
}

impl Render for Complete {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin_in(window);
        let screen = self.screen.clone();
        let Some(view) = self.view.upgrade() else {
            return div().into_any_element();
        };
        let rows = finished(view.read(cx));
        self.listed = rows.clone();
        let several = rows.len() > 1;
        let never = self.never;
        let root = crate::widgets::native::dialog("complete", t("Download complete"), theme, cx)
            .track_focus(&self.focus)
            // Return presses Close, the default, as Escape does: the window
            // opens of its own accord, over whatever has the keys, and a
            // Return meant for another dialog must not run the file that just
            // finished. Running it takes a click on Open.
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter"
                    && crate::widgets::native::return_is_default(window, Some(&this.focus), cx)
                {
                    this.close(window, cx);
                }
            }))
            .child(crate::widgets::fit::fitted(
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
                            .gap(px(space::L))
                            .children(
                                rows.iter()
                                    .enumerate()
                                    .map(|(i, row)| file_row(i, row, theme, &screen, cx)),
                            ),
                    )
                    .child(native::buttons(
                        vec![native::switch_inline(
                            "never",
                            t("Don’t show this window again").into(),
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
                            native::button("close", t("Close").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)))
                                .into_any_element(),
                        ],
                    )),
            ));
        crate::window::safe::kept_clear(root, window).into_any_element()
    }
}

/// One finished file in Download complete: its name, size and place at the start, its Open and
/// Open folder at the end, on the name's lines.
fn file_row(
    i: usize,
    row: &Finished,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Complete>,
) -> gpui::Div {
    let name = row.name.clone();
    let size = format::bytes(row.size);
    let (id, open_path, folder_path) = (row.id, row.path.clone(), row.path.clone());
    // One file: its name, size and place at the start, its two
    // buttons at the end, on the name's lines. Three buttons under
    // each file (twelve for four) made every file as loud as the
    // next; Open with… is on the row's menu (unslop review).
    // Where the name and the buttons do not fit on one line (a
    // phone, a large text size), the buttons go under it.
    div()
        .flex()
        .map(crate::window::mirror::row)
        .flex_wrap()
        .items_center()
        .gap(px(space::L))
        .child(
            div()
                .flex_grow(1.0)
                .flex_shrink(1.0)
                .flex_basis(px(NAME_BASIS))
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(space::XXS))
                .child(
                    div()
                        .flex()
                        .map(crate::window::mirror::row)
                        .items_center()
                        .gap(px(space::M))
                        .child(
                            native::cut(("done-name", i), name.clone(), false, screen)
                                .flex_1()
                                .font_weight(gpui::FontWeight::SEMIBOLD),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(theme.muted)
                                .child(screen.label(size)),
                        ),
                )
                // Its place on one line, cut short as its name is: a
                // long path wrapped over five lines (Windows'
                // C:\Users\…), and the window showed two files where
                // it holds three.
                .child(
                    native::cut(("done-path", i), row.path.clone(), true, screen)
                        .text_size(px(native::SMALL_TEXT))
                        .text_color(theme.muted),
                ),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .map(crate::window::mirror::row)
                .gap(px(native::LABEL_GAP))
                .child(
                    native::button(("open", i), t("Open").into(), false, theme, screen).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.act(id, open_path.clone(), true, cx)
                        }),
                    ),
                )
                .child(
                    native::button(("folder", i), t("Open folder").into(), false, theme, screen)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.act(id, folder_path.clone(), false, cx)
                        })),
                ),
        )
}

/// Opens "Download complete", in the corner above the status bar.
pub fn open(view: WeakEntity<Downloads>, cx: &mut App) -> Option<gpui::AnyWindowHandle> {
    let downloads = view.upgrade()?;
    let (width, height) = crate::widgets::native::WindowSize::Complete.here();
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    let opened = crate::window::screen::open_window(
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
            crate::widgets::native::close_is_cancel(&complete, window, cx);
            cx.new(|cx| gpui_component::Root::new(complete, window, cx))
        },
    );
    opened.ok().map(Into::into)
}

impl crate::widgets::native::CloseButton for Complete {
    fn close_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
    }
}
