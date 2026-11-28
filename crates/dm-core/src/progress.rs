// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! How fast a download is going, and how long is left.
//!
//! Kept apart from the fetching because it is arithmetic over samples, and
//! arithmetic over samples is worth testing without a socket. The engine's
//! only job is to count bytes; deciding what that means is here.

use std::time::Duration;

/// What a caller needs to draw a row.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Progress {
    /// Bytes on disk so far.
    pub downloaded: u64,
    /// `None` where the server never said how big the file is.
    pub total: Option<u64>,
    /// Bytes per second, smoothed.
    pub speed: f64,
    /// `None` when there is nothing to base one on.
    pub eta: Option<Duration>,
    /// `total` is an estimate, not the server's word: a stream's size from the pieces fetched
    /// so far and how many it has (gaps row Y22). Shown as "≈".
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub estimated: bool,
}

impl Progress {
    /// How far along, from zero to one, where that is knowable.
    pub fn fraction(&self) -> Option<f64> {
        match self.total {
            Some(total) if total > 0 => Some((self.downloaded as f64 / total as f64).min(1.0)),
            _ => None,
        }
    }
}

/// Turns a running byte count into a rate.
///
/// Smoothed rather than instantaneous. A raw sample swings with every chunk
/// boundary and stall, and a figure that jitters between 2 MB/s and 30 MB/s
/// twice a second is worse than useless — people read it as the download
/// being broken.
#[derive(Debug)]
pub struct Meter {
    last_bytes: u64,
    speed: f64,
    total: Option<u64>,
    /// Whether `total` is an estimate (`set_total`).
    estimated: bool,
    /// How far the figure has come from its start: 0 before any byte moved, 1 after the first
    /// interval that brought some, 2 from then on. That first interval is partial, its start
    /// spent on the request before the first byte, so the next interval replaces its figure
    /// rather than being averaged with it. Averaged, a slow start (a loaded computer's request
    /// taking 150 ms of the first 250) showed four fifths of a 500 KB/s limit at one second and
    /// took a second and a half more to come up to it (gaps row 196).
    warm: u8,
}

/// How much of the new sample to believe. The rest is carried over, which is
/// what makes the number readable rather than merely current.
const WEIGHT: f64 = 0.3;

impl Meter {
    /// A meter for a file of `total` bytes (unknown where `None`), started at `started_at` seconds.
    #[must_use]
    pub fn new(total: Option<u64>, started_at: u64) -> Self {
        Meter {
            last_bytes: started_at,
            speed: 0.0,
            total,
            estimated: false,
            warm: 0,
        }
    }

    /// The total from now on, and whether it is an estimate: a stream's grows surer as its
    /// pieces come (gaps row Y22).
    pub fn set_total(&mut self, total: Option<u64>, estimated: bool) {
        self.total = total;
        self.estimated = estimated && total.is_some();
    }

    /// Records where the count has reached, `elapsed` since the last sample.
    ///
    /// A sample taken over no time at all says nothing about rate — dividing
    /// by it would produce an infinity that then poisons the average — so it
    /// updates the count and leaves the speed alone.
    pub fn sample(&mut self, downloaded: u64, elapsed: Duration) -> Progress {
        let seconds = elapsed.as_secs_f64();
        if seconds > 0.0 {
            let moved = downloaded.saturating_sub(self.last_bytes) as f64;
            let instant = moved / seconds;
            // The first interval's figure is replaced by the next that brings bytes, not by one
            // that brings none: a loaded computer's quiet moment after the first bytes set a
            // finished download's rate to nothing (a loaded run of the tests, 2026-11-28).
            self.speed = if self.speed == 0.0 || (self.warm == 1 && moved > 0.0) {
                instant
            } else {
                self.speed * (1.0 - WEIGHT) + instant * WEIGHT
            };
            if moved > 0.0 && self.warm < 2 {
                self.warm += 1;
            }
        }
        self.last_bytes = downloaded;

        Progress {
            downloaded,
            total: self.total,
            speed: self.speed,
            eta: self.eta(downloaded),
            estimated: self.estimated,
        }
    }

    /// Time left at the current rate.
    ///
    /// Withheld rather than guessed where the answer would be meaningless: no
    /// known total, or a rate of zero, would each give an ETA of infinity or
    /// of nothing, and a row showing either is a row telling a lie.
    fn eta(&self, downloaded: u64) -> Option<Duration> {
        let total = self.total?;
        let left = total.checked_sub(downloaded)?;
        if left == 0 {
            return Some(Duration::ZERO);
        }
        if self.speed <= 1.0 {
            return None;
        }
        Some(Duration::from_secs_f64(left as f64 / self.speed))
    }
}

#[cfg(test)]
mod tests;
