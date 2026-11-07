// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A finished download's toast gives Windows' handles back (gaps row 156,
//! BUGLOG BUG-012). notify-rust's toasts each kept two semaphores and an
//! event for as long as dm ran, so a long session grew by about three
//! handles a finished download. The toasts here go straight to the Action
//! Center, with no banner, and are taken out of it again: nothing is shown.

#![cfg(windows)]

use dm_desktop::toast::{heard, remove_group, show_unseen, MOST_HEARD};

fn handles() -> i64 {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("(Get-Process -Id {}).HandleCount", std::process::id()),
        ])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

const GROUP: &str = "dm-handles-test";

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
    std::thread::sleep(std::time::Duration::from_secs(3));
    let start = handles();
    plain(30, "plain");
    std::thread::sleep(std::time::Duration::from_secs(2));
    let after_plain = handles();
    with_a_click(30, "a file");
    std::thread::sleep(std::time::Duration::from_secs(2));
    let after_click = handles();
    let still_heard = heard();
    let _ = remove_group(GROUP);
    println!(
        "HANDLES at rest {start}; 30 plain toasts {:+}; 30 with a click {:+}; {still_heard} heard",
        after_plain - start,
        after_click - after_plain
    );
    assert!(
        still_heard <= MOST_HEARD,
        "{still_heard} toasts still heard"
    );
    // notify-rust's kept three each, 90 for thirty, either kind.
    assert!(
        after_plain - start <= 10,
        "30 toasts with nothing to open kept {} handles",
        after_plain - start
    );
    assert!(
        after_click - after_plain <= 10,
        "30 toasts with a file to open kept {} handles",
        after_click - after_plain
    );
}
