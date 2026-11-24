// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A wall-clock limit that allows for how loaded the machine is now (gaps rows N33, 221): a test
//! that says "within 300 ms" failed when other builds held the processors, though dm did nothing
//! wrong. The machine's load is read as `bench/` reads a runner's speed, by measuring it: how
//! late a thread asked to sleep 2 ms is woken, which is about on time on an idle machine and
//! several times late on a busy one.

use std::time::{Duration, Instant};

/// The most a limit is stretched: past this the machine is too busy to judge timing at all,
/// and a failure is still a failure.
const MOST: f64 = 8.0;

/// How loaded this machine is now, as a factor of at least 1: the worse of how much longer a
/// busy loop takes on the wall clock than the processor time it was given (a thread sharing
/// its processor gets a share of it; macOS and Linux, from the thread's own clock), and how
/// late a thread asked to sleep 2 ms is woken.
pub fn load_factor() -> f64 {
    shared().max(woken_late()).clamp(1.0, MOST)
}

/// Wall time over processor time of about 5 ms of work; 1 where the thread's clock is not read.
fn shared() -> f64 {
    #[cfg(unix)]
    {
        let cpu = || {
            let mut t = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            // SAFETY: `t` is a valid timespec the call writes into.
            unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
            Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
        };
        let (wall, began) = (Instant::now(), cpu());
        let mut x = 0u64;
        while cpu().saturating_sub(began) < Duration::from_millis(5) {
            for i in 0..1000u64 {
                x = std::hint::black_box(x.wrapping_mul(6364136223846793005).wrapping_add(i));
            }
        }
        std::hint::black_box(x);
        let given = cpu().saturating_sub(began);
        wall.elapsed().as_secs_f64() / given.as_secs_f64().max(1e-6)
    }
    #[cfg(not(unix))]
    {
        1.0
    }
}

/// The worst of five 2 ms sleeps over 2 ms, a millisecond late counted as on time.
fn woken_late() -> f64 {
    let asked = Duration::from_millis(2);
    let worst = (0..5)
        .map(|_| {
            let began = Instant::now();
            std::thread::sleep(asked);
            began.elapsed()
        })
        .max()
        .unwrap_or(asked);
    let late = worst.saturating_sub(Duration::from_millis(1));
    late.as_secs_f64() / asked.as_secs_f64()
}

/// `limit` stretched by [`load_factor`]: as asked on an idle machine, up to [`MOST`] times it on
/// a loaded one.
pub fn allowing_for_load(limit: Duration) -> Duration {
    limit.mul_f64(load_factor())
}

/// [`allowing_for_load`], the load the worse of `before` (a [`load_factor`] read as the timed
/// work began) and now: a load that eased by the check still stretched the work.
pub fn allowing_for_load_since(limit: Duration, before: f64) -> Duration {
    limit.mul_f64(load_factor().max(before).clamp(1.0, MOST))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_limit_is_never_shortened_nor_stretched_past_the_most() {
        let limit = Duration::from_millis(300);
        let allowed = allowing_for_load(limit);
        assert!(allowed >= limit);
        assert!(allowed <= limit.mul_f64(MOST));
    }
}
