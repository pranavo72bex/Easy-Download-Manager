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

use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, SharedString, Window, WindowBounds,
    WindowOptions,
};

/// The main window's view.
struct Main {
    title: SharedString,
}

impl Render for Main {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .justify_center()
            .items_center()
            .child(self.title.clone())
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
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
        cx.activate(true);
    });
}
