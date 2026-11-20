// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The words of the ongoing notification while dm is busy (gaps row N6): how many download and
//! wait, and the speed of them all. Put up by `android/busy.rs`; here, where the host's tests
//! reach it.

use crate::{Row, RowState};

/// The ongoing notification's title and body for `rows`, the list now.
pub(crate) fn summary(rows: &[Row]) -> (String, String) {
    let running = rows.iter().filter(|r| r.state == RowState::Running).count();
    let waiting = rows.iter().filter(|r| r.state == RowState::Queued).count();
    let speed: f64 = rows
        .iter()
        .filter(|r| r.state == RowState::Running)
        .map(|r| r.speed.max(0.0))
        .sum();
    let title = match running {
        0 if waiting > 0 => "dm: waiting to download".to_owned(),
        // Busy with neither: a finished torrent is being shared.
        0 => "dm is sharing finished torrents".to_owned(),
        1 => "dm: 1 download running".to_owned(),
        n => format!("dm: {n} downloads running"),
    };
    let mut body = if running > 0 {
        rate(speed)
    } else {
        String::new()
    };
    if waiting > 0 {
        if !body.is_empty() {
            body.push_str(" · ");
        }
        body.push_str(&format!("{waiting} waiting"));
    }
    (title, body)
}

/// A speed in the units the desktop's window shows (`dm_core::KB` to the kilobyte). The
/// desktop's own words for it (its `view::format::rate`, translated) come to the phone with
/// the shared view code (gaps row N15).
fn rate(per_second: f64) -> String {
    let k = dm_core::KB as f64;
    let mut value = per_second.max(0.0);
    if value < k {
        return format!("{} B/s", value.round());
    }
    let mut unit = "KB/s";
    value /= k;
    for next in ["MB/s", "GB/s"] {
        if (value * 10.0).round() / 10.0 < k {
            break;
        }
        value /= k;
        unit = next;
    }
    format!("{value:.1} {unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(state: RowState, speed: f64) -> Row {
        Row {
            id: 1,
            url: "https://example.com/a".into(),
            name: "a".into(),
            state,
            failure: None,
            downloaded: 0,
            total: None,
            estimated: false,
            speed,
            eta_seconds: None,
            path: String::new(),
            category: None,
            torrent: false,
            swarm: None,
        }
    }

    #[test]
    fn the_ongoing_notification_counts_and_adds_the_speeds() {
        let k = dm_core::KB as f64;
        let rows = [
            row(RowState::Running, 1.5 * k * k),
            row(RowState::Running, 0.5 * k * k),
            row(RowState::Queued, 0.0),
            row(RowState::Completed, 9.0 * k * k),
        ];
        assert_eq!(
            summary(&rows),
            (
                "dm: 2 downloads running".to_owned(),
                "2.0 MB/s · 1 waiting".to_owned()
            )
        );
        assert_eq!(
            summary(&[row(RowState::Running, 512.0)]),
            ("dm: 1 download running".to_owned(), "512 B/s".to_owned())
        );
        assert_eq!(
            summary(&[row(RowState::Queued, 0.0)]).0,
            "dm: waiting to download"
        );
        assert_eq!(summary(&[]).0, "dm is sharing finished torrents");
    }
}
