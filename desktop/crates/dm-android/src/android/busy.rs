// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! While dm is busy (gaps row N6): the ongoing notification, changed at most once a second and
//! only when its words change, and the partial wake lock; both let go when nothing downloads,
//! waits or is shared, and when the engine is gone.

use std::sync::{Arc, Weak};
use std::time::Duration;

use super::notify;
use crate::Engine;

/// Watches `engine` on a thread of its own for as long as it runs.
pub(crate) fn watch(engine: &Arc<Engine>) -> Result<(), String> {
    let engine = Arc::downgrade(engine);
    std::thread::Builder::new()
        .name("dm-busy".into())
        .spawn(move || run(&engine))
        .map(drop)
        .map_err(|e| format!("dm could not watch its downloads: {e}"))
}

fn run(engine: &Weak<Engine>) {
    let mut shown: Option<(String, String)> = None;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let Some(running) = engine.upgrade() else {
            break;
        };
        let now = running
            .busy()
            .then(|| crate::ongoing::summary(&running.list()));
        drop(running);
        if now == shown {
            continue;
        }
        // A refusal (no permission to notify yet) is tried again on the next change.
        match &now {
            Some((title, body)) => {
                let _ = notify::ongoing(title, body);
                let _ = notify::stay_awake(true);
            }
            None => {
                let _ = notify::end_ongoing();
                let _ = notify::stay_awake(false);
            }
        }
        shown = now;
    }
    let _ = notify::end_ongoing();
    let _ = notify::stay_awake(false);
}
