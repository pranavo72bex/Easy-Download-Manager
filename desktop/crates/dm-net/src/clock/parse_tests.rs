// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The dates dm reads, on the edges of chrono's grammar: each case is what chrono 0.4.45 took
//! or refused (gaps row 215).

use super::*;

/// RFC 9110's example, `Sun, 06 Nov 1994 08:49:37 GMT`, in seconds since 1970.
const EXAMPLE: i64 = 784_111_777;

fn unix_2822(text: &str) -> Option<i64> {
    rfc2822(text).map(|at| at.unix)
}

#[test]
fn an_http_date_reads_in_every_shape_chrono_took() {
    for text in [
        "Sun, 06 Nov 1994 08:49:37 GMT",
        "06 Nov 1994 08:49:37 GMT",
        "Sun, 6 Nov 1994 08:49:37 GMT",
        "sun, 06 nov 1994 08:49:37 gmt",
        "  Sun, 06 Nov 1994 08:49:37 GMT",
        "Sun,06  Nov   1994  08:49:37   GMT",
        "Sun, 06 Nov 1994 08 : 49 :37 GMT",
        "Sun, 06 Nov 94 08:49:37 GMT",
        "Sun, 06 Nov 094 08:49:37 GMT",
        "Sun, 06 Nov 1994 08:49:37 +0000",
        "Sun, 06 Nov 1994 08:49:37 -0000",
        "Sun, 06 Nov 1994 08:49:37 UT",
        "Sun, 06 Nov 1994 08:49:37 Z",
        "Sun, 06 Nov 1994 08:49:37 a",
        "Sun, 06 Nov 1994 08:49:37 GMT (the server's)",
        "Sun, 06 Nov 1994 08:49:37 GMT(a (nested) \\) one) ()",
    ] {
        assert_eq!(unix_2822(text), Some(EXAMPLE), "{text:?}");
    }
    assert_eq!(unix_2822("Sun, 06 Nov 1994 08:49 GMT"), Some(EXAMPLE - 37));
    assert_eq!(
        unix_2822("Thu, 06 Nov 14 08:49:37 GMT"),
        unix_2822("Thu, 06 Nov 2014 08:49:37 GMT")
    );
    assert!(unix_2822("06 Nov 0654 08:49:37 GMT").is_some_and(|u| u < 0));
}

#[test]
fn an_http_dates_zone_moves_it_as_chrono_moved_it() {
    for (zone, east) in [
        ("+0100", 3_600),
        ("-0130", -5_400),
        ("+2359", 86_340),
        ("EDT", -4 * 3_600),
        ("est", -5 * 3_600),
        ("CDT", -5 * 3_600),
        ("CST", -6 * 3_600),
        ("MDT", -6 * 3_600),
        ("MST", -7 * 3_600),
        ("PDT", -7 * 3_600),
        ("PST", -8 * 3_600),
    ] {
        let text = format!("Sun, 06 Nov 1994 08:49:37 {zone}");
        assert_eq!(unix_2822(&text), Some(EXAMPLE - east), "{text}");
    }
}

#[test]
fn an_http_date_chrono_refused_is_refused() {
    for text in [
        "",
        "Sun, 06 Nov 1994 08:49:37 GMT ",
        "Mon, 06 Nov 1994 08:49:37 GMT",
        "Sun 06 Nov 1994 08:49:37 GMT",
        "Sunday, 06-Nov-94 08:49:37 GMT",
        "Sun Nov  6 08:49:37 1994",
        "06 November 1994 08:49:37 GMT",
        "06 Nov 1994 08:49:37",
        "06 Nov 1994 08:49:37 CET",
        "06 Nov 1994 08:49:37 J",
        "06 Nov 1994 08:49:37 +00:00",
        "06 Nov 1994 08:49:37 +000",
        "06 Nov 1994 08:49:37 +0060",
        "06 Nov 1994 08:49:37 +2400",
        "06 Nov 1994 08:49:37 \u{2212}0100",
        "06 Nov 1994 24:00:00 GMT",
        "06 Nov 1994 08:60:00 GMT",
        "06 Nov 1994 08:49:61 GMT",
        "06 Nov 1994 08:49: 37 GMT",
        "06 Nov 1994 8:49:37 GMT",
        "00 Nov 1994 08:49:37 GMT",
        "31 Nov 1994 08:49:37 GMT",
        "06 Nov 4 08:49:37 GMT",
        "06 Nov 1994 08:49:37 GMT (open",
        "06 Nov 1994 08:49:37 GMT x",
    ] {
        assert_eq!(rfc2822(text), None, "{text:?}");
    }
}

#[test]
fn a_leap_second_is_the_second_before_and_a_second_more() {
    let leap = rfc2822("Wed, 31 Dec 2008 23:59:60 GMT");
    assert_eq!(
        leap,
        Some(Stamp {
            unix: 1_230_767_999,
            nanos: 1_000_000_000
        })
    );
    let leap = rfc3339("1998-12-31T23:59:60Z").unwrap();
    assert_eq!(leap.unix, 915_148_799);
    assert_eq!(leap.unix_millis(), 915_148_800_000);
}

#[test]
fn an_rfc3339_time_reads_as_chrono_read_it() {
    let base = rfc3339("1996-12-20T00:39:57Z").unwrap();
    for text in [
        "1996-12-19T16:39:57-08:00",
        "1996-12-19T16:39:57\u{2212}08:00",
        "1996-12-20t00:39:57z",
        "1996-12-20 00:39:57+00:00",
        "1996-12-20T05:39:57+05:00",
    ] {
        assert_eq!(rfc3339(text), Some(base), "{text:?}");
    }
    let half = rfc3339("1996-12-20T00:39:57.5Z").unwrap();
    assert_eq!(half.unix_millis(), base.unix_millis() + 500);
    let long = rfc3339("1996-12-20T00:39:57.123456789123Z").unwrap();
    assert_eq!(long.nanos, 123_456_789);
    assert!(rfc3339("1996-12-20T00:39:57+23:59").is_some());
    for text in [
        "1996-12-20T00:39:57",
        "1996-12-20T00:39Z",
        "1996-12-20T00:39:57+0000",
        "1996-12-20T00:39:57+24:00",
        "1996-12-20T00:39:57+05:60",
        "1996-12-20T00:39:57Z ",
        " 1996-12-20T00:39:57Z",
        "1996-12-20T00:39:57.Z",
        "1996-13-20T00:39:57Z",
        "1997-02-29T00:39:57Z",
        "1996-12-20T24:00:00Z",
        "1996-12-20T23:59:61Z",
        "+1996-12-20T00:39:57Z",
        "1996-1-20T00:39:57Z",
    ] {
        assert_eq!(rfc3339(text), None, "{text:?}");
    }
}

#[test]
fn a_dash_time_with_no_zone_reads_as_chrono_read_it() {
    for (text, same) in [
        ("2026-11-12T10:00:00", "2026-11-12T10:00:00Z"),
        ("2026-1-2T3:4:5", "2026-01-02T03:04:05Z"),
        ("+2026-11-12T10:00:00", "2026-11-12T10:00:00Z"),
        ("2026-11-12T 10:00:00", "2026-11-12T10:00:00Z"),
    ] {
        assert!(iso_utc(text).is_some(), "{text:?}");
        assert_eq!(iso_utc(text), rfc3339(same), "{text:?}");
    }
    let whole = rfc3339("2026-11-12T10:00:00Z").unwrap().unix_millis();
    let quarter = iso_utc("2026-11-12T10:00:00.250").unwrap();
    assert_eq!(quarter.unix_millis(), whole + 250);
    for text in [
        "2026-11-12t10:00:00",
        "2026-11-12T10:00",
        "2026-11-12T10:00:00Z",
        "2026-11-12T10:00:00.",
        "20261-11-12T10:00:00",
        "2026-11-12T10:00:61",
    ] {
        assert_eq!(iso_utc(text), None, "{text:?}");
    }
}

#[test]
fn a_schedule_date_reads_as_chrono_read_it() {
    let date = |y, m, d| Date::new(y, m, d);
    assert_eq!(ymd("2026-12-31"), date(2026, 12, 31));
    assert_eq!(ymd("2026-1-5"), date(2026, 1, 5));
    assert_eq!(ymd(" 2026- 1- 5"), date(2026, 1, 5));
    assert_eq!(ymd("+2026-12-31"), date(2026, 12, 31));
    assert_eq!(ymd("26-12-31"), date(26, 12, 31));
    assert_eq!(ymd("-0001-01-01"), date(-1, 1, 1));
    for text in [
        "",
        "2026-12-31 ",
        "2026/12/31",
        "2026-02-30",
        "2026-12",
        "2026-12-31x",
        "12026-01-01",
    ] {
        assert_eq!(ymd(text), None, "{text:?}");
    }
    let at = |hour, minute| date(2026, 12, 31).map(|d| (d, hour, minute));
    assert_eq!(ymd_hm("2026-12-31 23:30"), at(23, 30));
    assert_eq!(ymd_hm("2026-12-3123:30"), at(23, 30));
    assert_eq!(ymd_hm("2026-12-31\t 1:5"), at(1, 5));
    for text in [
        "2026-12-31 24:00",
        "2026-12-31 23:60",
        "2026-12-31 23:30:00",
        "2026-12-31",
        "2026-12-31 23",
    ] {
        assert_eq!(ymd_hm(text), None, "{text:?}");
    }
}
