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
            self.speed = if self.speed == 0.0 || self.warm == 1 {
                instant
            } else {
                self.speed * (1.0 - WEIGHT) + instant * WEIGHT
            };
            if self.warm == 1 || (self.warm == 0 && moved > 0.0) {
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
mod tests {
    use super::*;

    fn secs(n: f64) -> Duration {
        Duration::from_secs_f64(n)
    }

    /// A stream's estimated size is carried and marked, and gives a time left (gaps row Y22).
    #[test]
    fn an_estimated_total_is_marked_and_gives_a_time_left() {
        let mut m = Meter::new(None, 0);
        assert_eq!(m.sample(100, secs(1.0)).eta, None);
        m.set_total(Some(1000), true);
        let p = m.sample(200, secs(1.0));
        assert!(p.estimated);
        assert_eq!(p.total, Some(1000));
        assert!(p.eta.is_some());
        m.set_total(None, true);
        assert!(
            !m.sample(300, secs(1.0)).estimated,
            "no total, nothing estimated"
        );
    }

    #[test]
    fn the_first_sample_is_the_rate_it_measured() {
        let mut m = Meter::new(Some(1000), 0);
        let p = m.sample(100, secs(1.0));
        assert_eq!(p.speed, 100.0);
        assert_eq!(p.downloaded, 100);
    }

    #[test]
    fn a_spike_moves_the_figure_without_becoming_it() {
        let mut m = Meter::new(Some(10_000), 0);
        m.sample(50, secs(1.0)); // the first interval, partial
        m.sample(150, secs(1.0)); // settled at 100/s
        let p = m.sample(1150, secs(1.0)); // a 1000/s burst

        assert!(p.speed > 100.0, "the rise should register");
        assert!(p.speed < 1000.0, "but not be believed whole");
    }

    #[test]
    fn the_first_interval_that_brought_bytes_is_replaced_not_averaged() {
        // Nothing in the first look; then an interval whose start went on the request.
        let mut m = Meter::new(Some(10_000_000), 0);
        assert_eq!(m.sample(0, secs(0.25)).speed, 0.0);
        assert_eq!(m.sample(50_000, secs(0.25)).speed, 200_000.0);
        // The first whole interval is the figure: 500 KB/s at once, not 290 KB/s.
        assert_eq!(m.sample(175_000, secs(0.25)).speed, 500_000.0);
        // And from then on it is smoothed.
        let p = m.sample(425_000, secs(0.25)).speed;
        assert!(p > 500_000.0 && p < 1_000_000.0, "{p}");
    }

    #[test]
    fn a_sample_over_no_time_does_not_produce_an_infinity() {
        let mut m = Meter::new(Some(1000), 0);
        m.sample(100, secs(1.0));
        let p = m.sample(200, Duration::ZERO);

        assert!(p.speed.is_finite(), "speed became {}", p.speed);
        assert_eq!(p.downloaded, 200, "the count still moves");
    }

    #[test]
    fn eta_falls_as_the_file_arrives() {
        let mut m = Meter::new(Some(1000), 0);
        let early = m.sample(100, secs(1.0)).eta.unwrap();
        let later = m.sample(800, secs(1.0)).eta.unwrap();
        assert!(later < early, "{later:?} should be less than {early:?}");
    }

    #[test]
    fn no_eta_is_offered_when_there_is_nothing_to_base_one_on() {
        // A stalled transfer: any number here would be invented.
        let mut m = Meter::new(Some(1000), 0);
        assert_eq!(m.sample(0, secs(1.0)).eta, None);

        // And a server that never said how big the file is.
        let mut unknown = Meter::new(None, 0);
        assert_eq!(unknown.sample(500, secs(1.0)).eta, None);
    }

    #[test]
    fn a_finished_download_has_no_time_left_rather_than_none_at_all() {
        let mut m = Meter::new(Some(1000), 0);
        assert_eq!(m.sample(1000, secs(1.0)).eta, Some(Duration::ZERO));
    }

    #[test]
    fn resuming_does_not_count_staged_bytes_as_just_arrived() {
        // Started with 900 already on disk: the rate is what arrived since,
        // not a burst of 900 in the first second.
        let mut m = Meter::new(Some(1000), 900);
        assert_eq!(m.sample(950, secs(1.0)).speed, 50.0);
    }

    #[test]
    fn fraction_is_withheld_when_the_total_is_unknown() {
        let p = Progress {
            downloaded: 5,
            total: None,
            speed: 0.0,
            eta: None,
            estimated: false,
        };
        assert_eq!(p.fraction(), None);

        let known = Progress {
            downloaded: 250,
            total: Some(1000),
            speed: 0.0,
            eta: None,
            estimated: false,
        };
        assert_eq!(known.fraction(), Some(0.25));
    }

    #[test]
    fn fraction_does_not_run_past_the_end() {
        // A server that sends more than it promised should not draw a bar
        // past its own end.
        let p = Progress {
            downloaded: 1200,
            total: Some(1000),
            speed: 0.0,
            eta: None,
            estimated: false,
        };
        assert_eq!(p.fraction(), Some(1.0));
    }
}
