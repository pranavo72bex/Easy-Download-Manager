// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::Outside;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use zbus::zvariant::Value;

/// The launcher dm is installed with (`dm-gpui.desktop`).
const APP: &str = "application://dm-gpui.desktop";

pub(super) struct Entry(Option<Sender<Outside>>);

impl Entry {
    pub(super) fn start() -> Entry {
        let (send, take) = channel::<Outside>();
        let started = std::thread::Builder::new()
            .name("dm-launcher".into())
            .spawn(move || {
                let Ok(bus) = zbus::blocking::Connection::session() else {
                    return;
                };
                while let Ok(mut now) = take.recv() {
                    // Only the latest of a burst is sent.
                    while let Ok(later) = take.try_recv() {
                        now = later;
                    }
                    let _ = bus.emit_signal(
                        None::<&str>,
                        "/com/canonical/unity/launcherentry/dm",
                        "com.canonical.Unity.LauncherEntry",
                        "Update",
                        &(APP, properties(now)),
                    );
                }
            });
        Entry(started.ok().map(|_| send))
    }

    pub(super) fn show(&self, now: Outside) {
        if let Some(send) = &self.0 {
            let _ = send.send(now);
        }
    }
}

/// What the signal says of `now`.
pub(super) fn properties(now: Outside) -> HashMap<&'static str, Value<'static>> {
    let mut said = HashMap::new();
    said.insert(
        "progress",
        Value::from(f64::from(now.percent.unwrap_or(0)) / 100.0),
    );
    said.insert("progress-visible", Value::from(now.percent.is_some()));
    said.insert("count", Value::from(now.running as i64));
    said.insert("count-visible", Value::from(now.running > 0));
    said
}
