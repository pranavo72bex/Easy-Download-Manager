// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The phone's window, drawn with Slint, the same on a phone and on a computer (`src/main.rs`).
//! For now a placeholder that shows dm is running and how many rows its list has: the screens
//! (gaps row N7: the list, a row's details, Add, Options) replace [`run`]'s window, keeping its
//! signature, and [`refresh`] stays the one way the engine asks for a redraw.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use slint::ComponentHandle;

use crate::{Engine, RowState};

/// The window's Slint component, private to this module: what is public is [`run`].
mod ui {
    slint::slint! {
        #[edition = "1.19"]
        export component Shell inherits Window {
            in property <string> heading: "dm";
            in property <string> line;
            title: "dm";
            preferred-width: 412px;
            preferred-height: 915px;
            VerticalLayout {
                padding: 24px;
                spacing: 12px;
                alignment: start;
                Text {
                    text: root.heading;
                    font-size: 32px;
                }
                Text {
                    text: root.line;
                    font-size: 18px;
                    wrap: word-wrap;
                }
            }
        }
    }
}

use ui::Shell;

thread_local! {
    /// The window's redraw, on the window's thread, while [`run`] runs.
    static REDRAW: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
}

/// A redraw has been asked for and not yet made: a burst of changes from the engine's threads
/// makes one.
static PENDING: AtomicBool = AtomicBool::new(false);

/// Opens the window over `engine` and runs it until it is closed (on a phone, until the
/// activity is). Slint's platform must already be chosen: `slint::android::init` on a phone;
/// on a computer, Slint's own default.
pub fn run(engine: Arc<Engine>) -> Result<(), String> {
    let window = Shell::new().map_err(|e| format!("dm could not open its window: {e}"))?;
    let weak = window.as_weak();
    let show = move || {
        if let Some(window) = weak.upgrade() {
            window.set_line(count_line(&engine).into());
        }
    };
    show();
    REDRAW.with(|redraw| *redraw.borrow_mut() = Some(Box::new(show)));
    let ran = window
        .run()
        .map_err(|e| format!("dm's window stopped: {e}"));
    REDRAW.with(|redraw| redraw.borrow_mut().take());
    ran
}

/// A window saying why dm could not start, until it is closed.
pub fn failed(why: &str) -> Result<(), String> {
    let window = Shell::new().map_err(|e| format!("dm could not open its window: {e}"))?;
    window.set_heading("dm could not start".into());
    window.set_line(why.into());
    window
        .run()
        .map_err(|e| format!("dm's window stopped: {e}"))
}

/// Asks the window to draw the list again; called from any thread (the engine's
/// `AndroidSystem::changed`). Asks made before the window runs, or after, are let go.
pub fn refresh() {
    if PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    let sent = slint::invoke_from_event_loop(|| {
        PENDING.store(false, Ordering::Release);
        REDRAW.with(|redraw| {
            if let Some(show) = &*redraw.borrow() {
                show();
            }
        });
    });
    if sent.is_err() {
        PENDING.store(false, Ordering::Release);
    }
}

/// The placeholder's one line: how many rows, and how many of them download now.
fn count_line(engine: &Engine) -> String {
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
