// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The phone's window, drawn with GPUI, the desktop's toolkit (docs/decisions.md, "dm on
//! Android with GPUI"). For now gpui-android's first milestone (gaps row N16): a square in dm's
//! blue, "dm", and how many rows the engine's list has, drawn again whenever the engine says
//! the list changed. The view is plain GPUI, checked on the host too; only `android_main`
//! (`android.rs`) puts it on the phone's platform.
//!
//! N17 replaces [`Shell`] with dm-gpui's own window (`dm_gpui::window::screen::open_main_window`
//! and the narrow layout of N7), opened in [`open`]'s place over the same [`Engine`]; [`refresh`]
//! stays the one way the engine asks for a redraw.

use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use gpui::{
    div, px, rgb, App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window,
    WindowOptions,
};

use crate::{Engine, RowState};

/// The window's wake-up: the engine's threads send on it, the window's task draws again.
/// Bounded at one: a burst of changes makes one redraw.
static CHANGED: OnceLock<async_channel::Sender<()>> = OnceLock::new();

/// dm's blue, the square's colour.
const BLUE: u32 = 0x1e88e5;

/// The placeholder window's view: what it shows.
pub struct Shell {
    shown: Shown,
}

/// What the placeholder shows: the engine's list, or why dm could not start.
enum Shown {
    Engine(Arc<Engine>),
    Failed(String),
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let (heading, line) = match &self.shown {
            Shown::Engine(engine) => ("dm", count_line(engine)),
            Shown::Failed(why) => ("dm could not start", why.clone()),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(24.))
            .bg(rgb(0xffffff))
            .text_color(rgb(0x1b1b1f))
            .child(div().size(px(48.)).rounded(px(8.)).bg(rgb(BLUE)))
            .child(div().text_size(px(32.)).child(heading))
            .child(div().text_size(px(18.)).child(line))
    }
}

/// Opens the window over `engine`, filling the phone's screen; called in the application's
/// launch (`gpui::Application::run`). `resumed` is called each time the window becomes active
/// again (the activity resumed: a link may have been shared meanwhile). Where it fails the
/// error says why, in dm's words.
pub fn open(
    engine: Arc<Engine>,
    resumed: Option<Rc<dyn Fn()>>,
    cx: &mut App,
) -> Result<(), String> {
    let (sender, receiver) = async_channel::bounded::<()>(1);
    // A second open (android_main run again in the same process) keeps the first sender, whose
    // receiver is gone: its sends then fail and are let go, as before a window exists.
    let _ = CHANGED.set(sender);
    cx.open_window(WindowOptions::default(), move |window, cx| {
        cx.new(|cx| {
            cx.spawn(async move |this, cx| {
                while receiver.recv().await.is_ok() {
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
            .detach();
            if let Some(resumed) = resumed {
                cx.observe_window_activation(window, move |_, window, _| {
                    if window.is_window_active() {
                        resumed();
                    }
                })
                .detach();
            }
            Shell {
                shown: Shown::Engine(engine),
            }
        })
    })
    .map(|_| ())
    .map_err(|e| format!("dm could not open its window: {e}"))
}

/// A window saying why dm could not start.
pub fn failed(why: &str, cx: &mut App) -> Result<(), String> {
    let why = why.to_owned();
    cx.open_window(WindowOptions::default(), move |_, cx| {
        cx.new(|_| Shell {
            shown: Shown::Failed(why),
        })
    })
    .map(|_| ())
    .map_err(|e| format!("dm could not open its window: {e}"))
}

/// Asks the window to draw the list again; called from any thread (the engine's
/// `AndroidSystem::changed`). Asks made before the window is open, or after it closed, are let
/// go.
pub fn refresh() {
    if let Some(changed) = CHANGED.get() {
        let _ = changed.try_send(());
    }
}

/// The placeholder's one line: how many rows, and how many of them download now.
pub fn count_line(engine: &Engine) -> String {
    let rows = engine.list();
    let running = rows
        .iter()
        .filter(|row| row.state == RowState::Running)
        .count();
    let listed = match rows.len() {
        0 => "No downloads yet".to_owned(),
        1 => "1 download".to_owned(),
        n => format!("{n} downloads"),
    };
    if running > 0 {
        format!("{listed}, {running} running")
    } else {
        listed
    }
}
