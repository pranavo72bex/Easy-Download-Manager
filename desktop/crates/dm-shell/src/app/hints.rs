// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The hints dm gives once ever: when each is due, and the setting that
//! marks it given. What a hint says, and where, is the presentation's.

use std::sync::Arc;

use dm_core::{Entry, State};

use crate::Settings;

/// A download this big (5 GiB) is worth scheduling for the night.
pub const BIG: u64 = 5 << 30;

/// A hint given once ever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// A big download is running: the Scheduler can run it at night.
    BigDownload,
    /// A download is running and its progress window does not open by
    /// itself: Details shows it.
    Details,
}

impl Hint {
    /// Whether the hint is due now: not given before, and the rows call
    /// for it. `line_free` is whether the presentation has nothing else to
    /// say in the place the hint would go (only Details waits for it).
    pub fn due(self, settings: &Settings, rows: &[Arc<Entry>], line_free: bool) -> bool {
        match self {
            Hint::BigDownload => {
                !settings.hinted_big
                    && rows.iter().any(|r| {
                        r.state == State::Running && r.progress.total.is_some_and(|t| t >= BIG)
                    })
            }
            Hint::Details => {
                !settings.hinted_details
                    && !settings.show_progress
                    && line_free
                    && rows.iter().any(|r| r.state == State::Running)
            }
        }
    }

    /// Marks the hint given in `settings`, to be saved.
    pub fn mark(self, settings: &mut Settings) {
        match self {
            Hint::BigDownload => settings.hinted_big = true,
            Hint::Details => settings.hinted_details = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(state: State, total: Option<u64>) -> Arc<Entry> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": 1,
                "url": "http://example.com/f.bin",
                "state": serde_json::to_value(state).unwrap(),
                "progress": {"downloaded": 0, "total": total, "speed": 0.0, "eta": null},
                "filename": "f.bin",
            }))
            .unwrap(),
        )
    }

    #[test]
    fn the_big_download_hint_is_due_once_for_a_running_download_of_five_gigabytes() {
        let mut settings = Settings::default();
        let big = [row(State::Running, Some(BIG))];
        assert!(Hint::BigDownload.due(&settings, &big, false));
        assert!(!Hint::BigDownload.due(&settings, &[row(State::Running, Some(BIG - 1))], true));
        assert!(!Hint::BigDownload.due(&settings, &[row(State::Paused, Some(BIG))], true));
        assert!(!Hint::BigDownload.due(&settings, &[row(State::Running, None)], true));
        Hint::BigDownload.mark(&mut settings);
        assert!(!Hint::BigDownload.due(&settings, &big, true), "given once");
        assert!(!settings.hinted_details, "and only that one marked");
    }

    #[test]
    fn the_details_hint_waits_for_a_free_line_and_not_where_progress_opens_by_itself() {
        let mut settings = Settings {
            show_progress: false,
            ..Settings::default()
        };
        let running = [row(State::Running, None)];
        assert!(Hint::Details.due(&settings, &running, true));
        assert!(
            !Hint::Details.due(&settings, &running, false),
            "something else is said"
        );
        assert!(!Hint::Details.due(&settings, &[row(State::Queued, None)], true));
        settings.show_progress = true;
        assert!(
            !Hint::Details.due(&settings, &running, true),
            "progress opens by itself"
        );
        settings.show_progress = false;
        Hint::Details.mark(&mut settings);
        assert!(!Hint::Details.due(&settings, &running, true), "given once");
    }
}
