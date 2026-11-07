// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A finished download's toast gives Windows' handles back (gaps row 156,
//! BUGLOG BUG-012). notify-rust's toasts each kept two semaphores and an
//! event for as long as dm ran, so a long session grew by about three
//! handles a finished download. The toasts here go straight to the Action
//! Center, with no banner, and are taken out of it again: nothing is shown.
//!
//! A kept handle a toast grows with every toast, round after round; what
//! Windows' own worker threads and late releases hold comes and goes and
//! does not. So the toasts go in rounds, the count is read once things have
//! settled, and the limit is half a handle a toast over all the rounds: far
//! above the coming and going (+15 for thirty, seen once beside the whole
//! suite), far below one handle kept a toast.

#![cfg(windows)]

use dm_desktop::toast::{heard, remove_group, show_unseen, MOST_HEARD};
use std::time::Duration;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};

/// The handles this process holds now, as Task Manager counts them.
fn handles() -> i64 {
    let mut count = 0u32;
    // SAFETY: the current process's pseudo-handle needs no closing; `count`
    // is written when the call succeeds.
    let counted = unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) };
    assert_ne!(counted, 0, "{}", std::io::Error::last_os_error());
    i64::from(count)
}

/// The fewest handles held over a second, once a second has passed: a
/// handle Windows is about to let go is not counted.
fn settled() -> i64 {
    std::thread::sleep(Duration::from_secs(1));
    (0..10)
        .map(|_| {
            std::thread::sleep(Duration::from_millis(100));
            handles()
        })
        .min()
        .unwrap()
}

const GROUP: &str = "dm-handles-test";
const ROUND: usize = 30;
const ROUNDS: usize = 3;

fn plain(n: usize, what: &str) {
    for i in 0..n {
        show_unseen::<fn()>("dm test", &format!("{what} {i}"), None, GROUP).unwrap();
    }
}

fn with_a_click(n: usize, what: &str) {
    for i in 0..n {
        show_unseen("dm test", &format!("{what} {i}"), Some(|| {}), GROUP).unwrap();
    }
}

#[test]
fn toasts_give_their_handles_back() {
    let _ = remove_group(GROUP);
    if let Err(e) = show_unseen::<fn()>("dm test", "first", None, GROUP) {
        eprintln!("this computer shows no toasts ({e}): nothing to measure");
        return;
    }
    // What Windows opens once, for the first toasts and the first events
    // they send back (about twenty handles), is not the toasts'.
    plain(10, "warming up");
    with_a_click(30, "warming up");
    let start = settled();
    let after_plain: Vec<i64> = (0..ROUNDS)
        .map(|round| {
            plain(ROUND, &format!("plain {round}"));
            settled()
        })
        .collect();
    let after_click: Vec<i64> = (0..ROUNDS)
        .map(|round| {
            with_a_click(ROUND, &format!("a file {round}"));
            settled()
        })
        .collect();
    let still_heard = heard();
    let _ = remove_group(GROUP);
    let middle = after_plain[ROUNDS - 1];
    let end = after_click[ROUNDS - 1];
    println!(
        "HANDLES at rest {start}; after each {ROUND} plain toasts {after_plain:?}; \
         after each {ROUND} with a click {after_click:?}; {still_heard} heard"
    );
    assert!(
        still_heard <= MOST_HEARD,
        "{still_heard} toasts still heard"
    );
    // notify-rust's kept three each, 270 for ninety, either kind.
    let toasts = (ROUND * ROUNDS) as i64;
    let most = toasts / 2;
    assert!(
        middle - start <= most,
        "{toasts} toasts with nothing to open kept {} handles (from {start}: {after_plain:?})",
        middle - start
    );
    assert!(
        end - middle <= most,
        "{toasts} toasts with a file to open kept {} handles (from {middle}: {after_click:?})",
        end - middle
    );
}
