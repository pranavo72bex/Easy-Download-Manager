// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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

/// A quiet interval after the first bytes (a loaded computer, or a file already whole) keeps
/// a rate, decaying, rather than setting it to nothing; the next interval with bytes is then
/// the figure, as the first whole one is.
#[test]
fn a_quiet_interval_after_the_first_bytes_does_not_wipe_the_rate() {
    let mut m = Meter::new(Some(12_000_000), 0);
    assert_eq!(m.sample(3_000_000, secs(0.25)).speed, 12_000_000.0);
    let quiet = m.sample(3_000_000, secs(0.25)).speed;
    assert!(quiet > 0.0, "a quiet moment wiped the rate");
    assert_eq!(m.sample(4_000_000, secs(0.25)).speed, 4_000_000.0);
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
