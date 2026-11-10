// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A download's progress window, for every window (B53, B54, B63): how fast
//! each piece is coming in, what its title says, whether it can resume, a
//! torrent's swarm. What each window draws is its own.

use dm_core::{Entry, State};

use crate::format;
use crate::i18n::{t, tf};

/// How fast each piece of one download is coming in, worked out from where it
/// stood at the last two samples: the Connections section of its progress
/// window names a rate beside each piece under way.
#[derive(Default)]
pub struct ConnRates {
    at: Option<std::time::Instant>,
    /// By the piece's first byte: what it held at the last sample, and its
    /// rate since, bytes a second, smoothed.
    pieces: std::collections::HashMap<u64, (u64, f64)>,
}

impl ConnRates {
    /// The least time between two samples; any frame in between keeps the
    /// rates it has, so a rate is not worked out from a few bytes.
    const EVERY: std::time::Duration = std::time::Duration::from_millis(400);

    /// Takes a sample of `pieces`, as (first byte, bytes held).
    pub fn sample(&mut self, now: std::time::Instant, pieces: &[(u64, u64)]) {
        let Some(last) = self.at else {
            self.at = Some(now);
            self.pieces = pieces
                .iter()
                .map(|&(start, done)| (start, (done, 0.0)))
                .collect();
            return;
        };
        let secs = now.saturating_duration_since(last);
        if secs < Self::EVERY {
            return;
        }
        let secs = secs.as_secs_f64();
        let mut next = std::collections::HashMap::new();
        for &(start, done) in pieces {
            let rate = match self.pieces.get(&start) {
                // A piece that went on from where it was: half of what it
                // did just now and half of what it did before, as the
                // download's own speed is smoothed.
                Some(&(before, old)) if done >= before => {
                    let fresh = (done - before) as f64 / secs;
                    if old > 0.0 {
                        old * 0.5 + fresh * 0.5
                    } else {
                        fresh
                    }
                }
                // A piece not seen before, or one that went back (a repair):
                // nothing to say yet.
                _ => 0.0,
            };
            next.insert(start, (done, rate));
        }
        self.pieces = next;
        self.at = Some(now);
    }

    /// The piece's rate in bytes a second, `0.0` where it has none yet.
    pub fn rate(&self, start: u64) -> f64 {
        self.pieces.get(&start).map_or(0.0, |&(_, rate)| rate)
    }
}

/// What a torrent's progress window says of its swarm, line by line.
pub fn swarm_lines(swarm: &dm_core::Swarm) -> [String; 2] {
    [
        tf(
            "Peers: {} connected, {} known",
            &[&swarm.connected, &swarm.known],
        ),
        tf(
            "Sent to peers: {} at {}",
            &[
                &format::bytes(swarm.uploaded),
                &format::rate(swarm.upload_speed),
            ],
        ),
    ]
}

/// "Can resume", as the server said when the download was planned
/// (`resumable`, where it has said), not a guess from the size; nothing for
/// a download that is over.
pub fn can_resume_words(row: &Entry, resumable: Option<bool>) -> Option<&'static str> {
    if matches!(row.state, State::Completed | State::Cancelled) {
        return None;
    }
    let torrent = dm_core::is_torrent(&row.url);
    Some(match resumable {
        Some(true) => t("Yes"),
        Some(false) => t("No — a stop starts it again from the beginning"),
        // A torrent goes on from the pieces it has; not one that failed
        // before it had any.
        None if torrent
            && matches!(row.state, State::Failed(_))
            && row.progress.downloaded == 0 =>
        {
            t("No — nothing of it was fetched")
        }
        None if torrent => t("Yes, from the pieces it already has"),
        // The server answered, with a refusal.
        None if matches!(&row.state, State::Failed(why) if why.contains("the server answered")) => {
            t("No — the server refused it")
        }
        None if matches!(row.state, State::Failed(_)) => {
            t("Not known: it failed before the server answered")
        }
        None => t("Unknown until the server answers"),
    })
}

/// A piece of a download as its progress window lists it: done, waiting, so
/// much come of a length nobody said, or so much of its span (`None`: drawn
/// as a bar with those words).
pub fn piece_words(
    span: u64,
    done: u64,
    started: bool,
    running: bool,
    size_known: bool,
) -> (String, Option<f32>) {
    if done >= span {
        (tf("done, {}", &[&format::bytes(span)]), None)
    } else if done == 0 && !started && running {
        // Waiting only where no request for it is open yet.
        (tf("waiting, {}", &[&format::bytes(span)]), None)
    } else if !size_known || span >= 1 << 60 {
        // Of a length nobody said: what has come.
        (tf("{} so far", &[&format::bytes(done)]), None)
    } else {
        (
            tf("{} of {}", &[&format::bytes(done), &format::bytes(span)]),
            Some(done as f32 / span.max(1) as f32),
        )
    }
}

/// How far a download is, in whole percent: none while its size is unknown.
pub fn percent_of(downloaded: u64, total: Option<u64>) -> Option<u64> {
    let total = total.filter(|total| *total > 0)?;
    Some((u128::from(downloaded) * 100 / u128::from(total)).min(100) as u64)
}

/// A progress window's title with how far it is first (`37% Download
/// progress — name`), so a row of taskbar buttons or a window list tells the
/// downloads apart and how they are going; the title alone while the size is
/// unknown.
pub fn title_with_percent(title: String, downloaded: u64, total: Option<u64>) -> String {
    match percent_of(downloaded, total) {
        Some(percent) => format!("{percent}% {title}"),
        None => title,
    }
}

#[cfg(test)]
mod title_tests {
    use super::*;

    #[test]
    fn the_percentage_comes_first_and_follows_the_download() {
        assert_eq!(
            title_with_percent("Download progress — a.bin".into(), 370, Some(1000)),
            "37% Download progress — a.bin"
        );
        assert_eq!(
            title_with_percent("t".into(), 0, Some(1000)),
            "0% t",
            "a download that has not begun is at 0%"
        );
        assert_eq!(title_with_percent("t".into(), 1000, Some(1000)), "100% t");
    }

    #[test]
    fn no_size_means_no_percentage() {
        assert_eq!(title_with_percent("t".into(), 500, None), "t");
        assert_eq!(title_with_percent("t".into(), 500, Some(0)), "t");
    }

    #[test]
    fn more_than_the_size_is_a_hundred_not_more() {
        assert_eq!(percent_of(2_000, Some(1_000)), Some(100));
        assert_eq!(percent_of(u64::MAX, Some(1)), Some(100));
    }
}

#[cfg(test)]
mod conn_rate_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_piece_has_no_rate_until_it_has_been_seen_twice() {
        let t0 = Instant::now();
        let mut rates = ConnRates::default();
        rates.sample(t0, &[(0, 100)]);
        assert_eq!(rates.rate(0), 0.0);
        rates.sample(t0 + Duration::from_secs(1), &[(0, 1_100)]);
        assert!((rates.rate(0) - 1_000.0).abs() < 1.0);
        assert_eq!(rates.rate(99), 0.0);
    }

    #[test]
    fn a_frame_too_soon_keeps_the_rates_it_has() {
        let t0 = Instant::now();
        let mut rates = ConnRates::default();
        rates.sample(t0, &[(0, 0)]);
        rates.sample(t0 + Duration::from_secs(1), &[(0, 1_000)]);
        rates.sample(t0 + Duration::from_millis(1_100), &[(0, 9_999)]);
        assert!((rates.rate(0) - 1_000.0).abs() < 1.0);
    }

    #[test]
    fn a_piece_that_went_back_starts_again() {
        let t0 = Instant::now();
        let mut rates = ConnRates::default();
        rates.sample(t0, &[(0, 5_000)]);
        rates.sample(t0 + Duration::from_secs(1), &[(0, 6_000)]);
        rates.sample(t0 + Duration::from_secs(2), &[(0, 100)]);
        assert_eq!(rates.rate(0), 0.0);
    }
}

#[cfg(test)]
mod swarm_tests {
    use super::swarm_lines;

    #[test]
    fn a_torrents_peers_and_what_it_sent_are_said() {
        let lines = swarm_lines(&dm_core::Swarm {
            connected: 3,
            known: 12,
            uploaded: 2 * 1024 * 1024,
            upload_speed: 64.0 * 1024.0,
        });
        assert_eq!(lines[0], "Peers: 3 connected, 12 known");
        assert!(lines[1].starts_with("Sent to peers: 2"), "{}", lines[1]);
        assert!(lines[1].ends_with("/s"), "{}", lines[1]);
    }
}
