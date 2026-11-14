// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The heap's freed pages given back to the system at rest (gaps row 175), on every system:
//! apart from `memory`, which is macOS's alone.

/// Gives the heap's freed pages back to the system, at most every [`GIVE_BACK_EVERY`]: the
/// allocator keeps what is freed for its next use, and at rest dm held about 16 MB of heap
/// where about 6 MB was in use (macOS, 2026-11-13, gaps row 175). Called by the window while
/// nothing downloads, so no download pays for it. macOS: `malloc_zone_pressure_relief`; Linux
/// (glibc): `malloc_trim`; elsewhere nothing.
pub fn give_back() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let last = LAST.load(Ordering::Relaxed);
    if now.saturating_sub(last) < GIVE_BACK_EVERY.as_secs() {
        return;
    }
    LAST.store(now, Ordering::Relaxed);
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut libc::c_void, goal: usize) -> usize;
        }
        // SAFETY: a null zone asks every zone, a goal of 0 as much as each can give; the call
        // only returns pages the allocator holds free.
        unsafe {
            malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
        }
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // SAFETY: glibc's malloc_trim only releases memory its heap holds free.
        unsafe {
            libc::malloc_trim(0);
        }
    }
}

/// How often at most [`give_back`] works.
pub const GIVE_BACK_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(test)]
mod tests {
    /// Asked twice in a row, the second is passed over and neither fails, on every system.
    #[test]
    fn giving_back_twice_is_harmless() {
        super::give_back();
        super::give_back();
    }
}
