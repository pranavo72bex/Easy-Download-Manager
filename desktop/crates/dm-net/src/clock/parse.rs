// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The dates dm reads, to the grammar chrono 0.4 read them by (gaps row 215): what it took
//! dm takes, and what it refused dm refuses, so a server's `Date` or a schedule saved by an
//! older dm reads as it did. "Space" is any Unicode white space, as `str::trim_start` has it.

use super::{Date, Stamp, DAY, MONTHS, WEEKDAYS};

/// One part of a written date and time, as chrono's `%` items read it.
#[derive(Clone, Copy)]
enum Item {
    /// `%Y`: space, then a signed number of any length, or one to four digits.
    Year,
    /// `%m`: space, then one or two digits, 1 to 12.
    Month,
    /// `%d`: space, then one or two digits, 1 to 31.
    Day,
    /// `%H`: space, then one or two digits, 0 to 23.
    Hour,
    /// `%M`: space, then one or two digits, 0 to 59.
    Minute,
    /// `%S`: space, then one or two digits, 0 to 60 (a leap second).
    Second,
    /// `%.f`: nothing, or a dot and at least one digit, the tenth past the ninth let go.
    Fraction,
    /// A character written in the pattern, exactly.
    Literal(char),
    /// A space written in the pattern: any white space, none included.
    Space,
}

/// What the items of a pattern read.
#[derive(Default)]
struct Fields {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    nanos: u32,
}

/// `%Y-%m-%d`, as the Scheduler writes a date.
const YMD: [Item; 5] = [
    Item::Year,
    Item::Literal('-'),
    Item::Month,
    Item::Literal('-'),
    Item::Day,
];

/// `%Y-%m-%d %H:%M`, as a once-only schedule is kept.
const YMD_HM: [Item; 9] = [
    Item::Year,
    Item::Literal('-'),
    Item::Month,
    Item::Literal('-'),
    Item::Day,
    Item::Space,
    Item::Hour,
    Item::Literal(':'),
    Item::Minute,
];

/// `%Y-%m-%dT%H:%M:%S%.f`, an `xs:dateTime` with no zone.
const ISO: [Item; 12] = [
    Item::Year,
    Item::Literal('-'),
    Item::Month,
    Item::Literal('-'),
    Item::Day,
    Item::Literal('T'),
    Item::Hour,
    Item::Literal(':'),
    Item::Minute,
    Item::Literal(':'),
    Item::Second,
    Item::Fraction,
];

/// A date written `YYYY-MM-DD` (chrono's `NaiveDate::parse_from_str(text, "%Y-%m-%d")`).
pub fn ymd(text: &str) -> Option<Date> {
    let f = fields(text, &YMD)?;
    Date::new(f.year, f.month, f.day)
}

/// A date and a time of day written `YYYY-MM-DD HH:MM` (chrono's
/// `NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M")`): the date, the hour and the minute.
pub fn ymd_hm(text: &str) -> Option<(Date, u32, u32)> {
    let f = fields(text, &YMD_HM)?;
    Some((Date::new(f.year, f.month, f.day)?, f.hour, f.minute))
}

/// An `xs:dateTime` with no zone, `2026-11-12T10:00:00.5`, taken as UTC (chrono's
/// `NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")`, then `and_utc`).
pub fn iso_utc(text: &str) -> Option<Stamp> {
    let f = fields(text, &ISO)?;
    let date = Date::new(f.year, f.month, f.day)?;
    stamp(date, f.hour, f.minute, f.second, f.nanos, 0)
}

/// An RFC 3339 date and time, `1996-12-19T16:39:57-08:00` (chrono's
/// `DateTime::parse_from_rfc3339`): `T`, `t` or a space between the date and the time, any
/// number of fraction digits, `Z`, `z` or an offset of `+`, `-` or `−` and `HH:MM`.
pub fn rfc3339(text: &str) -> Option<Stamp> {
    let bytes = text.as_bytes();
    let head = bytes.get(..19)?;
    let shape_holds = head.iter().enumerate().all(|(at, &c)| match at {
        4 | 7 => c == b'-',
        10 => matches!(c, b'T' | b't' | b' '),
        13 | 16 => c == b':',
        _ => c.is_ascii_digit(),
    });
    if !shape_holds {
        return None;
    }
    let two = |at: usize| -> u32 {
        let digit = |i: usize| head.get(i).map_or(0, |c| u32::from(c - b'0'));
        digit(at) * 10 + digit(at + 1)
    };
    let year = two(0) * 100 + two(2);
    let date = Date::new(i32::try_from(year).ok()?, two(5), two(8))?;
    let mut rest = text.get(19..)?;
    let mut nanos = 0;
    if let Some(after) = rest.strip_prefix('.') {
        (rest, nanos) = nanosecond(after)?;
    }
    let (rest, offset) = numeric_offset(rest, true, true)?;
    if !rest.is_empty() {
        return None;
    }
    stamp(date, two(11), two(14), two(17), nanos, offset)
}

/// An RFC 2822 date, which an HTTP date is (`Sun, 06 Nov 1994 08:49:37 GMT`), as chrono's
/// `DateTime::parse_from_rfc2822` read one: an optional weekday that must be the date's, a
/// two- or three-digit year from 1900 or 2000, the seconds optional, a zone by offset or by
/// name, comments in brackets after it, and nothing else, white space at the end included.
pub fn rfc2822(text: &str) -> Option<Stamp> {
    let mut s = text.trim_start();
    let mut weekday = None;
    if let Some((rest, day)) = short_name(s, &WEEKDAYS) {
        s = rest.strip_prefix(',')?;
        weekday = Some(day);
    }
    let (rest, day) = number(s.trim_start(), 1, 2)?;
    let (rest, month0) = short_name(space(rest)?, &MONTHS)?;
    let s = space(rest)?;
    let (rest, mut year) = number(s, 2, usize::MAX)?;
    match (s.len() - rest.len(), year) {
        (2, 0..=49) => year += 2000,
        (2, 50..=99) | (3, _) => year += 1900,
        _ => {}
    }
    let (rest, hour) = number(space(rest)?, 2, 2)?;
    let s = rest.trim_start().strip_prefix(':')?.trim_start();
    let (mut s, minute) = number(s, 2, 2)?;
    let mut second = 0;
    if let Some(after) = s.trim_start().strip_prefix(':') {
        (s, second) = number(after, 2, 2)?;
    }
    let (mut s, offset) = zone_2822(space(s)?)?;
    while let Some(rest) = comment(s) {
        s = rest;
    }
    if !s.is_empty() || !(1..=31).contains(&day) {
        return None;
    }
    let (year, day) = (i32::try_from(year).ok()?, u32::try_from(day).ok()?);
    let date = Date::new(year, month0 + 1, day)?;
    if weekday.is_some_and(|w| w != date.weekday_from_sunday()) {
        return None;
    }
    let (hour, minute, second) = (
        u32::try_from(hour).ok()?,
        u32::try_from(minute).ok()?,
        u32::try_from(second).ok()?,
    );
    stamp(date, hour, minute, second, 0, offset)
}

/// The moment of a date and a time of day written on a clock `offset` seconds east of UTC,
/// checked as chrono checked it: an hour below 24, a minute below 60, a second to 60 (a leap
/// second, the 59th and a billion nanoseconds), and an offset under a day.
fn stamp(
    date: Date,
    hour: u32,
    minute: u32,
    second: u32,
    nanos: u32,
    offset: i32,
) -> Option<Stamp> {
    if hour > 23 || minute > 59 || second > 60 || nanos > 999_999_999 {
        return None;
    }
    if i64::from(offset).abs() >= DAY {
        return None;
    }
    let (second, nanos) = if second == 60 {
        (59, nanos + 1_000_000_000)
    } else {
        (second, nanos)
    };
    let of_day = i64::from(hour * 3_600 + minute * 60 + second);
    let unix = date.days_since_epoch() * DAY + of_day - i64::from(offset);
    // The moment in UTC must be a date too.
    Date::from_days_since_epoch(unix.div_euclid(DAY))?;
    Some(Stamp { unix, nanos })
}

/// `text` read by `items`, all of it; `None` where any of it does not fit.
fn fields(text: &str, items: &[Item]) -> Option<Fields> {
    let mut s = text;
    let mut f = Fields::default();
    for item in items {
        match *item {
            Item::Literal(c) => s = s.strip_prefix(c)?,
            Item::Space => s = s.trim_start(),
            Item::Fraction => {
                if let Some(after) = s.strip_prefix('.') {
                    (s, f.nanos) = nanosecond(after)?;
                }
            }
            Item::Year => {
                let t = s.trim_start();
                let (rest, year) = if let Some(after) = t.strip_prefix('-') {
                    let (rest, v) = number(after, 1, usize::MAX)?;
                    (rest, 0i64.checked_sub(v)?)
                } else if let Some(after) = t.strip_prefix('+') {
                    number(after, 1, usize::MAX)?
                } else {
                    number(t, 1, 4)?
                };
                f.year = i32::try_from(year).ok()?;
                s = rest;
            }
            Item::Month => (s, f.month) = small(s, 1..=12)?,
            Item::Day => (s, f.day) = small(s, 1..=31)?,
            Item::Hour => (s, f.hour) = small(s, 0..=23)?,
            Item::Minute => (s, f.minute) = small(s, 0..=59)?,
            Item::Second => (s, f.second) = small(s, 0..=60)?,
        }
    }
    s.is_empty().then_some(f)
}

/// Space, then one or two digits within `range`.
fn small(s: &str, range: std::ops::RangeInclusive<u32>) -> Option<(&str, u32)> {
    let (rest, v) = number(s.trim_start(), 1, 2)?;
    let v = u32::try_from(v).ok().filter(|v| range.contains(v))?;
    Some((rest, v))
}

/// `min` to `max` ASCII digits at the start of `s`, as a number, and what follows them.
fn number(s: &str, min: usize, max: usize) -> Option<(&str, i64)> {
    let mut n: i64 = 0;
    let mut taken = 0;
    for &c in s.as_bytes().iter().take(max) {
        if !c.is_ascii_digit() {
            break;
        }
        n = n.checked_mul(10)?.checked_add(i64::from(c - b'0'))?;
        taken += 1;
    }
    if taken < min {
        return None;
    }
    s.get(taken..).map(|rest| (rest, n))
}

/// One to nine digits as nanoseconds (`5` is half a second), any digits after them let go.
fn nanosecond(s: &str) -> Option<(&str, u32)> {
    const SCALE: [u32; 10] = [
        0,
        100_000_000,
        10_000_000,
        1_000_000,
        100_000,
        10_000,
        1_000,
        100,
        10,
        1,
    ];
    let (rest, v) = number(s, 1, 9)?;
    let scale = SCALE.get(s.len() - rest.len()).copied()?;
    let nanos = u32::try_from(v).ok()?.checked_mul(scale)?;
    Some((rest.trim_start_matches(|c: char| c.is_ascii_digit()), nanos))
}

/// White space, at least some, and what follows it.
fn space(s: &str) -> Option<&str> {
    let rest = s.trim_start();
    (rest.len() < s.len()).then_some(rest)
}

/// One of `names` (three ASCII letters, any case) at the start of `s`: what follows it, and
/// its index.
fn short_name<'a>(s: &'a str, names: &[&str]) -> Option<(&'a str, u32)> {
    let head = s.as_bytes().get(..3)?;
    let at = names
        .iter()
        .position(|name| head.eq_ignore_ascii_case(name.as_bytes()))?;
    Some((s.get(3..)?, u32::try_from(at).ok()?))
}

/// An offset `+HHMM` or `-HHMM` (`+HH:MM` with `colon`, `−` for minus too with `minus_sign`,
/// and `Z` or `z` with both, as RFC 3339 has it): what follows it, and its seconds east.
fn numeric_offset(s: &str, colon: bool, minus_sign: bool) -> Option<(&str, i32)> {
    if let Some(rest) = s.strip_prefix(['Z', 'z']).filter(|_| colon) {
        return Some((rest, 0));
    }
    let (negative, s) = if let Some(rest) = s.strip_prefix('+') {
        (false, rest)
    } else if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else {
        let rest = s.strip_prefix('\u{2212}').filter(|_| minus_sign)?;
        (true, rest)
    };
    let digits = s.as_bytes();
    let (h1, h2) = (*digits.first()?, *digits.get(1)?);
    if !(h1.is_ascii_digit() && h2.is_ascii_digit()) {
        return None;
    }
    let mut s = s.get(2..)?;
    if colon {
        s = s.strip_prefix(':')?;
    }
    let digits = s.as_bytes();
    let (m1, m2) = (*digits.first()?, *digits.get(1)?);
    if !(matches!(m1, b'0'..=b'5') && m2.is_ascii_digit()) {
        return None;
    }
    let hours = i32::from(h1 - b'0') * 10 + i32::from(h2 - b'0');
    let minutes = i32::from(m1 - b'0') * 10 + i32::from(m2 - b'0');
    let seconds = hours * 3_600 + minutes * 60;
    Some((s.get(2..)?, if negative { -seconds } else { seconds }))
}

/// RFC 2822's zone: an offset `+HHMM`, `UT`, `GMT`, `Z`, the North American names (`EST`
/// to `PDT`), or a military letter (taken as UTC, as `-0000` says "unknown").
fn zone_2822(s: &str) -> Option<(&str, i32)> {
    let letters = s
        .bytes()
        .position(|c| !c.is_ascii_alphabetic())
        .unwrap_or(s.len());
    if letters == 0 {
        return numeric_offset(s, false, false);
    }
    let (name, rest) = s.split_at(letters);
    let hours = match name.to_ascii_lowercase().as_str() {
        "gmt" | "ut" | "z" => 0,
        "edt" => -4,
        "est" | "cdt" => -5,
        "cst" | "mdt" => -6,
        "mst" | "pdt" => -7,
        "pst" => -8,
        military if military.len() == 1 && military != "j" => 0,
        _ => return None,
    };
    Some((rest, hours * 3_600))
}

/// A comment in brackets after white space, `(…)`, nested brackets and `\` escapes in it:
/// what follows it.
fn comment(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'(') {
        return None;
    }
    let (mut depth, mut escaped) = (1usize, false);
    for (at, &c) in bytes.iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            b'\\' => escaped = true,
            b'(' => depth += 1,
            b')' if depth == 1 => return s.get(at + 1..),
            b')' => depth -= 1,
            _ => {}
        }
    }
    None
}
