// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! This computer's offset from UTC on Windows: the zone's rules for the moment's year in UTC
//! (`GetTimeZoneInformationForYear`), applied by the system (`SystemTimeToTzSpecificLocalTime`),
//! as chrono worked it out from the same rules.

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::Time::{
    GetTimeZoneInformationForYear, SystemTimeToTzSpecificLocalTime, TIME_ZONE_INFORMATION,
};

use super::{Date, LocalTime, DAY};

/// The offset from UTC, in seconds east, at `unix` (seconds since 1970 UTC); `None` outside
/// the years Windows' clock holds (1601 to 30827).
pub(super) fn offset_at(unix: i64) -> Option<i32> {
    let utc = LocalTime::at(unix, 0)?;
    let year = u16::try_from(utc.date.year())
        .ok()
        .filter(|y| (1601..=30827).contains(y))?;
    let field = |v: u32| u16::try_from(v).ok();
    let universal = SYSTEMTIME {
        wYear: year,
        wMonth: field(utc.date.month())?,
        wDayOfWeek: field(utc.weekday_from_sunday())?,
        wDay: field(utc.date.day())?,
        wHour: field(utc.hour)?,
        wMinute: field(utc.minute)?,
        wSecond: field(utc.second)?,
        wMilliseconds: 0,
    };
    let mut zone = TIME_ZONE_INFORMATION::default();
    let mut local = SYSTEMTIME::default();
    // SAFETY: each call is handed pointers to values on this stack, which outlive it; the
    // first writes only `zone` (a null zone asks for the current one), the second reads `zone`
    // and `universal` and writes only `local`, and neither keeps a pointer.
    let done = unsafe {
        GetTimeZoneInformationForYear(year, std::ptr::null(), &mut zone) != 0
            && SystemTimeToTzSpecificLocalTime(&zone, &universal, &mut local) != 0
    };
    if !done {
        return None;
    }
    let date = Date::new(
        i32::from(local.wYear),
        u32::from(local.wMonth),
        u32::from(local.wDay),
    )?;
    let shown = date.days_since_epoch() * DAY
        + i64::from(local.wHour) * 3_600
        + i64::from(local.wMinute) * 60
        + i64::from(local.wSecond);
    i32::try_from(shown - unix).ok()
}
