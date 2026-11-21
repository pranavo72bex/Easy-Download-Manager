// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! This computer's offset from UTC on macOS and Linux: the C library's `localtime_r`, which
//! reads the zone's rules (`TZ`, `/etc/localtime`) for any moment, past ones included.

use std::sync::atomic::{AtomicI64, Ordering};

extern "C" {
    /// POSIX's `tzset`, which the libc crate does not declare: the C library reads `TZ` and
    /// the zone file again, so a zone changed while dm runs is seen.
    fn tzset();
}

/// The second the zone was last read again, so it is read at most once a second, as chrono
/// read it: `localtime_r` alone keeps the zone it found first on Linux.
static ZONE_READ_AT: AtomicI64 = AtomicI64::new(i64::MIN);

/// The offset from UTC, in seconds east, at `unix` (seconds since 1970 UTC).
pub(super) fn offset_at(unix: i64) -> Option<i32> {
    let now = super::unix_now();
    let stale = ZONE_READ_AT.swap(now, Ordering::Relaxed) != now;
    // `time_t` is 64 bits on every system dm is built for.
    let time = unix as libc::time_t;
    // SAFETY: `tm` is plain integers and a pointer, for which all zeroes is a valid value.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: `tzset` touches only the C library's own zone state, under its own lock;
    // `localtime_r` reads `time` and writes only `tm`, both of which outlive the call, and
    // keeps no pointer to either.
    let done = unsafe {
        if stale {
            tzset();
        }
        libc::localtime_r(&time, &mut tm)
    };
    if done.is_null() {
        return None;
    }
    i32::try_from(tm.tm_gmtoff).ok()
}
