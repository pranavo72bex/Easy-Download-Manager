// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's own clock and calendar (gaps row 215): the time now, this computer's offset from UTC at
//! any moment (the system's own answer: `localtime_r` on macOS and Linux, the zone's rules for
//! the year on Windows), the days of the calendar, and the dates servers and people write
//! ([`rfc2822`], [`rfc3339`], [`iso_utc`], [`ymd`], [`ymd_hm`]), each read to the grammar the
//! chrono crate read them by, so every date that read before reads the same.

mod parse;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

pub use parse::{iso_utc, rfc2822, rfc3339, ymd, ymd_hm};

/// The earliest year a [`Date`] holds, as chrono's dates did.
pub const MIN_YEAR: i32 = (i32::MIN >> 13) + 1;
/// The latest year a [`Date`] holds, as chrono's dates did.
pub const MAX_YEAR: i32 = (i32::MAX >> 13) - 1;

/// Seconds in a day.
const DAY: i64 = 86_400;
/// The days from 0001-01-01 (day 1, as chrono counted) to 1970-01-01.
const EPOCH_FROM_CE: i64 = 719_163;

/// The months as an HTTP date writes them.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
/// The days as an HTTP date writes them, Sunday first.
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// A day of the (proleptic Gregorian) calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: i32,
    month: u8,
    day: u8,
}

impl Date {
    /// The date of `year`, `month` (1 to 12) and `day`; `None` where there is no such day, or
    /// the year is outside [`MIN_YEAR`] to [`MAX_YEAR`].
    pub fn new(year: i32, month: u32, day: u32) -> Option<Date> {
        if !(MIN_YEAR..=MAX_YEAR).contains(&year) || !(1..=12).contains(&month) {
            return None;
        }
        let last = days_in_month(year, month);
        let (month, day) = (u8::try_from(month).ok()?, u8::try_from(day).ok()?);
        (1..=last).contains(&day).then_some(Date { year, month, day })
    }

    /// The year.
    pub fn year(self) -> i32 {
        self.year
    }

    /// The month, 1 to 12.
    pub fn month(self) -> u32 {
        u32::from(self.month)
    }

    /// The month, 0 to 11.
    pub fn month0(self) -> u32 {
        u32::from(self.month) - 1
    }

    /// The day of the month, from 1.
    pub fn day(self) -> u32 {
        u32::from(self.day)
    }

    /// The days from 1970-01-01 (0) to this one, negative before it.
    pub fn days_since_epoch(self) -> i64 {
        let year = i64::from(self.year) - i64::from(self.month <= 2);
        let era = year.div_euclid(400);
        let of_era = year - era * 400;
        let march_based = (i64::from(self.month) + 9) % 12;
        let of_year = (153 * march_based + 2) / 5 + i64::from(self.day) - 1;
        let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
        era * 146_097 + of_cycle - 719_468
    }

    /// The date `days` after 1970-01-01; `None` outside the years a date holds.
    pub fn from_days_since_epoch(days: i64) -> Option<Date> {
        let shifted = days.checked_add(719_468)?;
        let era = shifted.div_euclid(146_097);
        let of_cycle = shifted - era * 146_097;
        let of_era = (of_cycle - of_cycle / 1_460 + of_cycle / 36_524 - of_cycle / 146_096) / 365;
        let of_year = of_cycle - (365 * of_era + of_era / 4 - of_era / 100);
        let march_based = (5 * of_year + 2) / 153;
        let day = of_year - (153 * march_based + 2) / 5 + 1;
        let month = if march_based < 10 {
            march_based + 3
        } else {
            march_based - 9
        };
        let year = of_era + era * 400 + i64::from(month <= 2);
        Date::new(
            i32::try_from(year).ok()?,
            u32::try_from(month).ok()?,
            u32::try_from(day).ok()?,
        )
    }

    /// The day counted from 0001-01-01 as day 1 (chrono's `num_days_from_ce`).
    pub fn days_from_ce(self) -> i64 {
        self.days_since_epoch() + EPOCH_FROM_CE
    }

    /// The day of the week, 0 = Sunday to 6 = Saturday.
    pub fn weekday_from_sunday(self) -> u32 {
        // 1970-01-01 was a Thursday.
        (self.days_since_epoch() + 4).rem_euclid(7) as u32
    }

    /// The date `days` on (back where negative); `None` outside the years a date holds.
    pub fn add_days(self, days: i64) -> Option<Date> {
        Date::from_days_since_epoch(self.days_since_epoch().checked_add(days)?)
    }
}

/// `YYYY-MM-DD`, a year outside 0 to 9999 with its sign, as chrono's `%Y-%m-%d` wrote it.
impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if (0..10_000).contains(&self.year) {
            write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
        } else {
            write!(f, "{:+05}-{:02}-{:02}", self.year, self.month, self.day)
        }
    }
}

/// The days of `month` in `year`.
fn days_in_month(year: i32, month: u32) -> u8 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// A moment as a date and a time of day on a clock `offset` seconds east of UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    /// The day on that clock.
    pub date: Date,
    /// The hour, 0 to 23.
    pub hour: u32,
    /// The minute, 0 to 59.
    pub minute: u32,
    /// The second, 0 to 59.
    pub second: u32,
    /// The clock's offset from UTC, in seconds east.
    pub offset: i32,
}

impl LocalTime {
    /// 1970-01-01 00:00 UTC, for a clock that cannot be read at all.
    const EPOCH: LocalTime = LocalTime {
        date: Date {
            year: 1970,
            month: 1,
            day: 1,
        },
        hour: 0,
        minute: 0,
        second: 0,
        offset: 0,
    };

    /// The moment `unix` (seconds since 1970 UTC) on a clock `offset` seconds east of UTC.
    pub fn at(unix: i64, offset: i32) -> Option<LocalTime> {
        let shown = unix.checked_add(i64::from(offset))?;
        let of_day = shown.rem_euclid(DAY);
        Some(LocalTime {
            date: Date::from_days_since_epoch(shown.div_euclid(DAY))?,
            hour: (of_day / 3_600) as u32,
            minute: (of_day / 60 % 60) as u32,
            second: (of_day % 60) as u32,
            offset,
        })
    }

    /// The minutes past this clock's midnight.
    pub fn minute_of_day(&self) -> u32 {
        self.hour * 60 + self.minute
    }

    /// The day of the week, 0 = Sunday to 6 = Saturday.
    pub fn weekday_from_sunday(&self) -> u32 {
        self.date.weekday_from_sunday()
    }

    /// The offset as chrono's `%:z` wrote it: `+05:30`, `-08:00`, `+00:00`, to the minute.
    pub fn offset_text(&self) -> String {
        let sign = if self.offset < 0 { '-' } else { '+' };
        let minutes = (self.offset.unsigned_abs() + 30) / 60;
        format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
    }
}

/// Whose clock a moment is read on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// This computer's, its summer time and its past rules as the system keeps them.
    Local,
    /// One a fixed number of seconds east of UTC.
    Fixed(i32),
}

impl Zone {
    /// UTC's own clock.
    pub const UTC: Zone = Zone::Fixed(0);

    /// The zone's offset from UTC, in seconds east, at `unix` (seconds since 1970 UTC); `None`
    /// where the system has no answer for that moment.
    pub fn offset_at(self, unix: i64) -> Option<i32> {
        match self {
            Zone::Local => system_offset_at(unix),
            Zone::Fixed(offset) => Some(offset),
        }
    }

    /// The moment `unix` (seconds since 1970 UTC) on this zone's clock.
    pub fn at(self, unix: i64) -> Option<LocalTime> {
        LocalTime::at(unix, self.offset_at(unix)?)
    }
}

#[cfg(unix)]
fn system_offset_at(unix: i64) -> Option<i32> {
    unix::offset_at(unix)
}

#[cfg(windows)]
fn system_offset_at(unix: i64) -> Option<i32> {
    windows::offset_at(unix)
}

/// A system dm is not built for keeps UTC.
#[cfg(not(any(unix, windows)))]
fn system_offset_at(_unix: i64) -> Option<i32> {
    Some(0)
}

/// The seconds since 1970-01-01 00:00 UTC now, rounded down; negative on a clock set before it.
pub fn unix_now() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
        Err(before) => {
            let before = before.duration();
            let whole = i64::try_from(before.as_secs()).unwrap_or(i64::MAX);
            -whole - i64::from(before.subsec_nanos() > 0)
        }
    }
}

/// Now on this computer's clock; UTC where the system cannot say its offset.
pub fn local_now() -> LocalTime {
    let now = unix_now();
    Zone::Local
        .at(now)
        .or_else(|| Zone::UTC.at(now))
        .unwrap_or(LocalTime::EPOCH)
}

/// `unix` (seconds since 1970 UTC) on this computer's clock, as it was then.
pub fn local_at(unix: i64) -> Option<LocalTime> {
    Zone::Local.at(unix)
}

/// A moment read from text: seconds since 1970 UTC, and the nanoseconds past them.
///
/// A leap second (`23:59:60`) is held as chrono held one: the second before, and a billion
/// nanoseconds or more past it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// Seconds since 1970-01-01 00:00 UTC.
    pub unix: i64,
    /// Nanoseconds past `unix`, below two billion.
    pub nanos: u32,
}

impl Stamp {
    /// Milliseconds since 1970 UTC (chrono's `timestamp_millis`).
    pub fn unix_millis(self) -> i64 {
        self.unix
            .saturating_mul(1_000)
            .saturating_add(i64::from(self.nanos / 1_000_000))
    }

    /// The whole seconds from `earlier` to this one, cut toward zero, a leap second counted as
    /// chrono counted one (its `(a - b).num_seconds()`).
    pub fn seconds_since(self, earlier: Stamp) -> i64 {
        let (days, of_day) = (self.unix.div_euclid(DAY), self.unix.rem_euclid(DAY));
        let (was_days, was_of_day) = (earlier.unix.div_euclid(DAY), earlier.unix.rem_euclid(DAY));
        let leap = |nanos: u32| i64::from(nanos >= 1_000_000_000);
        let adjust = match of_day.cmp(&was_of_day) {
            std::cmp::Ordering::Greater => leap(earlier.nanos),
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Less => -leap(self.nanos),
        };
        let seconds = i128::from(days - was_days) * i128::from(DAY)
            + i128::from(of_day - was_of_day)
            + i128::from(adjust);
        let nanos = seconds * 1_000_000_000 + i128::from(self.nanos) - i128::from(earlier.nanos);
        let whole = nanos / 1_000_000_000;
        let clamped = if whole < 0 { i64::MIN } else { i64::MAX };
        i64::try_from(whole).unwrap_or(clamped)
    }
}

/// `unix` (seconds since 1970 UTC) as an HTTP date: `Sun, 06 Nov 1994 08:49:37 GMT`; `None`
/// outside the years a date holds.
pub fn http_date(unix: i64) -> Option<String> {
    let at = LocalTime::at(unix, 0)?;
    let weekday = WEEKDAYS.get(at.weekday_from_sunday() as usize)?;
    let month = MONTHS.get(at.date.month0() as usize)?;
    Some(format!(
        "{weekday}, {:02} {month} {:04} {:02}:{:02}:{:02} GMT",
        at.date.day(),
        at.date.year(),
        at.hour,
        at.minute,
        at.second
    ))
}

#[cfg(test)]
mod parse_tests;
#[cfg(test)]
mod tests;
