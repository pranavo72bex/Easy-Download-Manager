// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's window in GPUI: the Phase B shell (docs/gaps.md G6). It opens the
//! main window and nothing of dm's yet; each check of
//! docs/migration/GPUI_MIGRATION_PLAN.md Phase B is made on it.

// As every dm crate: no unwrap, expect or panic in shipped code, an `unsafe`
// block says why it is sound, a string is built with `format!`.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
#![warn(missing_docs)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(clippy::string_add, clippy::needless_for_each)]

use std::time::Duration;

use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Role, SharedString, Window, WindowBounds,
    WindowHandle, WindowKind, WindowOptions,
};

/// The main window's view.
struct Main {
    title: SharedString,
}

impl Render for Main {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("main")
            .role(Role::Application)
            .aria_label(self.title.clone())
            .size_full()
            .flex()
            .justify_center()
            .items_center()
            .child(self.title.clone())
    }
}

/// A dialog in a window of its own, as dm's are (B42): a question, OK and
/// Cancel. Phase B's first check is whether a screen reader reaches both.
struct Question;

/// A button a screen reader can find: an id, the button role, its words.
fn button(id: &'static str, words: &'static str) -> impl IntoElement {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(words)
        .px_3()
        .py_1()
        .border_1()
        .rounded_md()
        .child(words)
}

impl Render for Question {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("question")
            .role(Role::Dialog)
            .aria_label("Delete the file?")
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child("Delete the file?")
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(button("ok", "OK").into_any_element())
                    .child(button("cancel", "Cancel").into_any_element()),
            )
    }
}

/// Opens the question in a dialog window of its own, over the screen's middle.
fn open_question(cx: &mut App) -> Option<WindowHandle<Question>> {
    let dialog = Bounds::centered(None, size(px(360.0), px(140.0)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(dialog)),
            kind: WindowKind::Dialog,
            ..Default::default()
        },
        |window, cx| {
            window.set_window_title("Delete");
            cx.new(|_| Question)
        },
    )
    .ok()
}

fn main() {
    let churn = std::env::args().any(|a| a == "--churn");
    let dump_accessibility = std::env::args().any(|a| a == "--accessibility");
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(640.0), px(400.0))),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("dm");
                cx.new(|_| Main { title: "dm".into() })
            },
        );
        if let Err(e) = opened {
            eprintln!("dm-gpui: the window could not be opened: {e}");
            cx.quit();
            return;
        }
        if open_question(cx).is_none() {
            eprintln!("dm-gpui: the dialog's window could not be opened");
        }
        cx.activate(true);
        if dump_accessibility {
            // What VoiceOver is given of each window, then out (Phase B
            // check 1). The first ask wakes AccessKit, which GPUI answers
            // with a placeholder and the real tree on the next frame: so
            // ask, draw every window again, then read.
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                cx.update(|cx| {
                    let _ = dm_desktop_base::a11y_tree::windows();
                    cx.refresh_windows();
                });
                cx.background_executor().timer(Duration::from_secs(1)).await;
                // With --churn, a dialog opened, read and closed 50 times
                // first: the tree must survive it (Phase B check 1).
                for _ in 0..if churn { 50 } else { 0 } {
                    let opened = cx.update(open_question);
                    cx.background_executor()
                        .timer(Duration::from_millis(80))
                        .await;
                    cx.update(|cx| {
                        let _ = dm_desktop_base::a11y_tree::windows();
                        if let Some(handle) = opened {
                            let _ = handle.update(cx, |_, window, _| window.remove_window());
                        }
                    });
                    cx.background_executor()
                        .timer(Duration::from_millis(40))
                        .await;
                }
                cx.update(|cx| cx.refresh_windows());
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                cx.update(|cx| {
                    match dm_desktop_base::a11y_tree::windows() {
                        Ok(tree) => println!("{tree:#}"),
                        Err(why) => eprintln!("dm-gpui: {why}"),
                    }
                    cx.quit();
                });
            })
            .detach();
        }
    });
}
