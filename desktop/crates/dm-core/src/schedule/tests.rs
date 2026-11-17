// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn the_next_start_is_the_window_or_a_queue_whichever_comes_first() {
    let schedule = Schedule {
        enabled: true,
        from: "23:00".into(),
        to: "07:00".into(),
        days: vec![5], // Friday nights only
        wake: false,
        when_done: String::new(),
        retries: 0,
        run_when_done: String::new(),
        hang_up: false,
        once_at: None,
        force: false,
    };
    // Monday 08:00: Friday 23:00 is four days and fifteen hours away.
    assert_eq!(
        minutes_until_next_start(&schedule, &[], 1, 8 * 60),
        Some(4 * 24 * 60 + 15 * 60)
    );
    // A queue starting daily at 02:00 comes first: tomorrow 02:00.
    assert_eq!(
        minutes_until_next_start(&schedule, &[120], 1, 8 * 60),
        Some(18 * 60)
    );
    // Nothing scheduled at all: nothing to wake for.
    assert_eq!(
        minutes_until_next_start(&Schedule::default(), &[], 1, 0),
        None
    );
    // Exactly at a start is not "next": the following one is.
    assert_eq!(
        minutes_until_next_start(&Schedule::default(), &[60], 2, 60),
        Some(24 * 60)
    );
}

/// Overnight, and therefore wrapping — which is the case the arithmetic
/// is for. The default window, 01:00 to 07:00, does not wrap at all.
fn nightly() -> Schedule {
    Schedule {
        enabled: true,
        from: "23:00".into(),
        to: "07:00".into(),
        days: (0..=6).collect(),
        wake: false,
        when_done: String::new(),
        retries: 0,
        run_when_done: String::new(),
        hang_up: false,
        once_at: None,
        force: false,
    }
}

#[test]
fn no_schedule_means_any_time() {
    let schedule = Schedule::default();
    assert!(!schedule.enabled);
    assert!(schedule.open_at(3, 12 * 60));
}

#[test]
fn a_window_that_wraps_midnight_is_open_on_both_sides_of_it() {
    let schedule = nightly();
    assert!(schedule.open_at(3, 23 * 60), "23:00 opens the window");
    assert!(schedule.open_at(3, 2 * 60), "02:00 is still inside it");
    assert!(!schedule.open_at(3, 12 * 60), "midday is not");
    assert!(!schedule.open_at(3, 7 * 60), "07:00 is the end, not inside");
}

#[test]
fn a_window_within_one_day_does_not_wrap() {
    let schedule = Schedule {
        from: "09:00".into(),
        to: "17:00".into(),
        ..nightly()
    };
    assert!(schedule.open_at(3, 12 * 60));
    assert!(!schedule.open_at(3, 8 * 60));
    assert!(!schedule.open_at(3, 18 * 60));
}

#[test]
fn the_small_hours_belong_to_the_day_the_window_opened() {
    /* A Friday-night window running to 07:00 is still the Friday window
     * at two on Saturday morning. Asking whether Saturday is scheduled
     * would close it halfway through. */
    let friday_only = Schedule {
        days: vec![5],
        ..nightly()
    };
    assert!(friday_only.open_at(5, 23 * 60), "Friday night");
    assert!(friday_only.open_at(6, 2 * 60), "Saturday 02:00 is Friday's");
    assert!(!friday_only.open_at(6, 23 * 60), "Saturday night is not");
}

#[test]
fn a_day_not_listed_is_closed() {
    let weekdays = Schedule {
        days: vec![1, 2, 3, 4, 5],
        ..nightly()
    };
    assert!(weekdays.open_at(3, 2 * 60));
    assert!(!weekdays.open_at(0, 2 * 60), "Sunday was not listed");
}

#[test]
fn no_day_ticked_means_every_day() {
    let any_day = Schedule {
        days: Vec::new(),
        ..nightly()
    };
    for day in 0..=6 {
        assert!(any_day.open_at(day, 2 * 60), "day {day} at 02:00");
        assert!(!any_day.open_at(day, 12 * 60), "day {day} at midday");
    }
}

#[test]
fn only_a_24_hour_time_is_a_time() {
    for good in ["00:00", "7:05", "23:59", " 01:00 "] {
        assert!(is_time(good), "{good}");
    }
    for bad in ["25:00", "1am", "12:60", "", "12", "ab:cd"] {
        assert!(!is_time(bad), "{bad}");
    }
}

#[test]
fn a_schedule_missing_fields_reads_with_defaults() {
    let read: Schedule = serde_json::from_str(r#"{"enabled": true}"#).unwrap_or_default();
    assert!(read.enabled);
    assert_eq!(read.from, Schedule::default().from);
}

#[test]
fn a_time_that_is_not_a_time_leaves_the_queue_running() {
    // Better than a queue that silently never runs because of a typo.
    let broken = Schedule {
        from: "not a time".into(),
        ..nightly()
    };
    assert!(broken.open_at(3, 12 * 60));
}

#[test]
fn an_empty_window_is_not_a_closed_one() {
    let empty = Schedule {
        from: "03:00".into(),
        to: "03:00".into(),
        ..nightly()
    };
    assert!(empty.open_at(3, 12 * 60));
}

fn once(at: &str) -> Schedule {
    Schedule {
        once_at: Some(at.into()),
        ..nightly()
    }
}

#[test]
fn a_once_only_date_opens_at_its_moment_and_closes_at_the_end_time() {
    let s = once("2026-11-03 23:00");
    let (nov3, nov4) = ((2026, 11, 3), (2026, 11, 4));
    assert!(!s.open_now(nov3, 2, 22 * 60 + 59), "a minute before");
    assert!(s.open_now(nov3, 2, 23 * 60), "at its moment");
    assert!(s.open_now(nov4, 3, 60), "past midnight, still inside");
    assert!(!s.open_now(nov4, 3, 7 * 60), "at the end time");
    assert!(!s.open_now((2026, 11, 2), 1, 23 * 60), "the day before");
}

#[test]
fn a_once_only_date_that_is_over_is_cleared_by_the_caller() {
    let s = once("2026-11-03 23:00");
    assert!(!s.once_over((2026, 11, 4), 6 * 60 + 59));
    assert!(s.once_over((2026, 11, 4), 7 * 60));
    assert!(!s.once_over((2026, 11, 3), 12 * 60), "not yet begun");
}

#[test]
fn a_once_only_window_within_one_day_ends_that_day() {
    let s = Schedule {
        to: "17:00".into(),
        ..once("2026-11-03 09:00")
    };
    assert!(s.open_now((2026, 11, 3), 2, 12 * 60));
    assert!(!s.open_now((2026, 11, 3), 2, 17 * 60));
    assert!(!s.open_now((2026, 11, 4), 3, 10 * 60), "not tomorrow");
}

#[test]
fn a_once_only_date_that_is_not_a_date_falls_back_to_the_weekly_window() {
    let s = once("tomorrow evening");
    assert_eq!(s.open_now((2026, 11, 3), 2, 2 * 60), s.open_at(2, 2 * 60));
    assert_eq!(s.open_now((2026, 11, 3), 2, 12 * 60), s.open_at(2, 12 * 60));
    assert!(!s.once_over((2026, 11, 9), 0));
}

#[test]
fn two_schedules_answer_each_their_own_window() {
    let night = nightly();
    let day = Schedule {
        from: "09:00".into(),
        to: "17:00".into(),
        ..nightly()
    };
    assert!(night.open_now((2026, 11, 3), 2, 2 * 60) && !day.open_now((2026, 11, 3), 2, 2 * 60));
    assert!(!night.open_now((2026, 11, 3), 2, 12 * 60) && day.open_now((2026, 11, 3), 2, 12 * 60));
}

#[test]
fn a_schedule_written_before_hang_up_leaves_the_connection_up() {
    let read: Schedule =
        serde_json::from_str(r#"{"enabled": true, "run_when_done": "x"}"#).unwrap();
    assert_eq!(read.run_when_done, "x");
    assert!(!read.hang_up, "hung up only when asked");
    assert!(!Schedule::default().hang_up);
}

#[test]
fn a_schedule_written_before_once_at_still_reads() {
    let read: Schedule =
        serde_json::from_str(r#"{"enabled": true, "from": "22:00"}"#).unwrap_or_default();
    assert_eq!(read.once_at, None);
    assert!(!read.force);
}

#[test]
fn asking_the_clock_agrees_with_asking_directly() {
    // Guards the wiring between `open` and `open_at`, which is the only
    // part `open_at`'s tests cannot see.
    let now = Local::now();
    let schedule = nightly();
    assert_eq!(
        schedule.open(),
        schedule.open_at(
            now.weekday().num_days_from_sunday() as u8,
            now.time().hour() * 60 + now.time().minute()
        )
    );
}
