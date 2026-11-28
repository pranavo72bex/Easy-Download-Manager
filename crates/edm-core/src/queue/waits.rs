// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every download held for the computer's own state (gaps row W11): a metered connection, or a
//! battery below what Options › Downloads allows. Held as the schedule holds them (paused,
//! `paused_by_schedule`, their parts kept), and put back by the same path once the state ends
//! (`may_run`, then `release_from_schedule`). The state itself is read by whoever drives EDM,
//! which says here what holds the queue; the queue reads no system.

use super::*;

/// What of the computer's state holds every download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Hold {
    /// The connection is metered (a phone's hotspot, a capped plan), and Options says to wait
    /// on one, or the person said to wait when asked.
    Metered,
    /// The computer runs on its battery, below the level Options says to wait under.
    Battery,
    /// A phone off Wi-Fi (and off a cable), where its Options say Wi-Fi only (gaps row N10).
    NotWifi,
    /// A phone roaming, where its Options say not while roaming (gaps row N10).
    Roaming,
    /// A phone's Data Saver on, where its Options say to wait while it is (gaps row N10).
    DataSaver,
}

impl Hold {
    /// Every hold, for what lists their words (the catalogs' check).
    pub const ALL: [Hold; 5] = [
        Hold::Metered,
        Hold::Battery,
        Hold::NotWifi,
        Hold::Roaming,
        Hold::DataSaver,
    ];

    /// Why the rows wait, in English, as a waiting row says it after "Waiting: ".
    #[must_use]
    pub fn said(self) -> &'static str {
        match self {
            Hold::Metered => "the connection is metered",
            Hold::Battery => "the battery is low",
            Hold::NotWifi => "the phone is not on Wi-Fi",
            Hold::Roaming => "the phone is roaming",
            Hold::DataSaver => "Data Saver is on",
        }
    }
}

impl DownloadList {
    /// Holds every download for `hold`, or, with `None`, lets them go on: what runs is paused
    /// and nothing starts at the next tick, as the schedule closing does; once the hold is
    /// lifted the rows it held are put back by the schedule's watcher (`may_run`). A download a
    /// person started outside the schedule goes on. Every finished torrent's sharing is held
    /// with them, and goes on when they do (gaps row N8).
    pub fn hold_for(&self, hold: Option<Hold>) {
        let before = std::mem::replace(&mut *crate::lock(&self.inner.held_by), hold);
        // A finished torrent's sharing sends over the same connection: held with them (N8).
        crate::torrent::hold_sharing(hold.is_some());
        if before != hold {
            self.kick();
        }
    }

    /// What of the computer's state holds every download now; `None` while nothing does.
    #[must_use]
    pub fn held_by(&self) -> Option<Hold> {
        *crate::lock(&self.inner.held_by)
    }
}
