// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The calendar's arithmetic, the clocks a moment is read on, and the HTTP date dm writes.

use super::*;

fn date(year: i32, month: u32, day: u32) -> Date {
    Date::new(year, month, day).unwrap()
}

#[test]
fn days_count_from_1970_and_from_the_first_day_as_chrono_counted() {
    assert_eq!(date(1970, 1, 1).days_since_epoch(), 0);
    assert_eq!(date(1969, 12, 31).days_since_epoch(), -1);
    assert_eq!(date(2000, 3, 1).days_since_epoch(), 11_017);
    assert_eq!(date(2026, 11, 19).days_since_epoch(), 20_776);
    assert_eq!(date(1, 1, 1).days_from_ce(), 1);
    assert_eq!(date(1970, 1, 1).days_from_ce(), 719_163);
    for days in (-800_000..800_000).step_by(997) {
        let back = Date::from_days_since_epoch(days).unwrap();
        assert_eq!(back.days_since_epoch(), days, "{back}");
        assert_eq!(back.add_days(1).unwrap().days_since_epoch(), days + 1);
    }
}

#[test]
fn a_day_that_is_not_one_is_refused() {
    assert!(Date::new(2000, 2, 29).is_some());
    assert!(Date::new(2024, 2, 29).is_some());
    assert!(Date::new(1900, 2, 29).is_none());
    assert!(Date::new(2023, 2, 29).is_none());
    assert!(Date::new(2026, 4, 31).is_none());
    assert!(Date::new(2026, 13, 1).is_none());
    assert!(Date::new(2026, 0, 1).is_none());
    assert!(Date::new(2026, 1, 0).is_none());
    assert!(Date::new(MAX_YEAR, 12, 31).is_some());
    assert!(Date::new(MAX_YEAR + 1, 1, 1).is_none());
    assert!(Date::new(MIN_YEAR, 1, 1).is_some());
    assert!(Date::new(MIN_YEAR - 1, 12, 31).is_none());
    assert_eq!(date(MAX_YEAR, 12, 31).add_days(1), None);
}

#[test]
fn weekdays_months_and_the_written_date() {
    assert_eq!(date(1970, 1, 1).weekday_from_sunday(), 4);
    assert_eq!(date(2026, 11, 19).weekday_from_sunday(), 4);
    assert_eq!(date(2026, 11, 22).weekday_from_sunday(), 0);
    assert_eq!(date(1600, 1, 1).weekday_from_sunday(), 6);
    assert_eq!(date(2024, 2, 28).add_days(1), Date::new(2024, 2, 29));
    assert_eq!(date(2024, 2, 28).add_days(2), Date::new(2024, 3, 1));
    assert_eq!(date(2026, 1, 1).add_days(-1), Date::new(2025, 12, 31));
    assert_eq!(date(2026, 3, 5).to_string(), "2026-03-05");
    assert_eq!(date(5, 1, 1).to_string(), "0005-01-01");
    assert_eq!(date(10_000, 1, 1).to_string(), "+10000-01-01");
    assert_eq!(date(-1, 1, 1).to_string(), "-0001-01-01");
    assert_eq!(date(2026, 3, 5).month0(), 2);
}

#[test]
fn a_moment_reads_on_any_clock() {
    let noon = 1_795_089_600;
    let at = LocalTime::at(noon, 0).unwrap();
    assert_eq!(at.date, date(2026, 11, 19));
    assert_eq!((at.hour, at.minute, at.second), (12, 0, 0));
    let at = Zone::Fixed(9 * 3_600).at(noon - 13 * 3_600).unwrap();
    assert_eq!(at.date, date(2026, 11, 19));
    assert_eq!((at.hour, at.minute_of_day()), (8, 480));
    let at = LocalTime::at(-1, 0).unwrap();
    assert_eq!(at.date, date(1969, 12, 31));
    assert_eq!((at.hour, at.second), (23, 59));
    let offset = |offset| LocalTime { offset, ..at }.offset_text();
    assert_eq!(offset(19_800), "+05:30");
    assert_eq!(offset(-28_800), "-08:00");
    assert_eq!(offset(-12_600), "-03:30");
    assert_eq!(offset(0), "+00:00");
}

#[test]
fn this_computers_clock_answers() {
    let now = local_now();
    assert!(now.offset.abs() <= 14 * 3_600, "{now:?}");
    assert!(local_at(0).is_some());
    assert!(Zone::Local.offset_at(unix_now()).is_some());
    let unix = unix_now();
    let back =
        now.date.days_since_epoch() * 86_400 + i64::from(now.hour * 3_600) - i64::from(now.offset);
    assert!((unix - back).abs() < 3_600 + 60, "{unix} against {back}");
}

#[test]
fn seconds_between_moments_count_a_leap_second_as_chrono_did() {
    let at = |unix, nanos| Stamp { unix, nanos };
    assert_eq!(at(100, 0).seconds_since(at(40, 0)), 60);
    assert_eq!(at(40, 0).seconds_since(at(100, 0)), -60);
    let leap = at(86_399, 1_000_000_000);
    assert_eq!(leap.seconds_since(at(86_399, 0)), 1);
    assert_eq!(leap.seconds_since(at(86_398, 0)), 2);
    assert_eq!(at(86_400, 0).seconds_since(leap), 0);
    assert_eq!(at(86_410, 0).seconds_since(leap), 10);
    assert_eq!(at(1, 500_000_000).seconds_since(at(0, 0)), 1);
    assert_eq!(at(0, 0).seconds_since(at(1, 500_000_000)), -1);
}

#[test]
fn an_http_date_is_written_and_read_back() {
    let example = 784_111_777;
    let text = http_date(example).unwrap();
    assert_eq!(text, "Sun, 06 Nov 1994 08:49:37 GMT");
    assert_eq!(rfc2822(&text).map(|at| at.unix), Some(example));
    for unix in (-2_000_000_000..4_000_000_000i64).step_by(7_777_777) {
        let text = http_date(unix).unwrap();
        assert_eq!(rfc2822(&text).map(|at| at.unix), Some(unix), "{text}");
    }
}
