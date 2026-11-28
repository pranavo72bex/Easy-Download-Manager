// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// A bar glides from where it last stood to its new width, and stands still where its
/// width has not changed (gaps row K7).
#[test]
fn a_bar_glides_from_its_last_width_to_its_new_one() {
    let fill = || div();
    let glide = |id| GLIDES.with(|g| g.borrow().get(&id).map(|&(from, to, _)| (from, to)));
    let _ = glided(7_000_001, 0.2, fill());
    assert_eq!(glide(7_000_001), Some((0.2, 0.2)), "a first width stands");
    let _ = glided(7_000_001, 0.5, fill());
    assert_eq!(glide(7_000_001), Some((0.2, 0.5)));
    // Drawn again with the same width (a hover, a tick with no news): the same glide.
    let _ = glided(7_000_001, 0.5, fill());
    assert_eq!(glide(7_000_001), Some((0.2, 0.5)));
    // A new width at once: from where the bar stands (hardly moved), not from 0.5, where
    // it has not got to (gaps row E21).
    let _ = glided(7_000_001, 0.9, fill());
    let (from, to) = glide(7_000_001).unwrap();
    assert!((0.2..0.3).contains(&from) && to == 0.9, "{from} to {to}");
}

/// A glide moves at one pace and is done in one tick of a running list.
#[test]
fn a_glide_is_linear_and_lasts_one_tick() {
    use std::time::Duration;
    assert_eq!(glide_at(0.2, 0.6, Duration::ZERO), 0.2);
    assert!((glide_at(0.2, 0.6, GLIDE / 2) - 0.4).abs() < 1e-4);
    assert_eq!(glide_at(0.2, 0.6, GLIDE), 0.6);
    assert_eq!(glide_at(0.2, 0.6, GLIDE * 3), 0.6);
    assert!(LEAVING_FOR < ARRIVING, "an exit is shorter than its enter");
}

/// A row fades in only where it was added after the list was first drawn, and only for a
/// moment: one the window opened with, or one scrolled into sight later, does not (K7).
#[test]
fn only_a_row_just_added_fades_in() {
    assert!(!just_added(8_000_001), "a row the window opened with");
    list_drawn();
    assert!(!just_added(8_000_001), "the same row drawn again");
    assert!(just_added(8_000_002), "a row added since");
    std::thread::sleep(ARRIVING + std::time::Duration::from_millis(50));
    assert!(!just_added(8_000_002), "drawn again once it has faded in");
}

/// Rows taken off the list fade out for a moment, and the list is read again after it
/// (K7); others are as they were.
#[test]
fn rows_taken_off_fade_out_for_a_moment() {
    let began = std::time::Instant::now();
    let until = leave(&[9_100_001]);
    assert!(until >= began + LEAVING_FOR);
    assert!(left(9_100_001).is_some_and(|done| done < 1.0));
    assert_eq!(left(9_100_002), None);
    std::thread::sleep(LEAVING_FOR);
    assert_eq!(left(9_100_001), Some(1.0));
    // Read off the list, then put back by Undo: seen again.
    gone(&[9_100_001]);
    assert_eq!(left(9_100_001), None);
}

/// A bar settles only where its download was seen running and has just finished: not one
/// finished when first seen, nor one stopped, nor once it has faded (K7).
#[test]
fn only_a_download_seen_finishing_settles() {
    assert!(
        !settling(9_000_001, false, true),
        "finished when first seen"
    );
    assert!(!settling(9_000_002, true, false), "running");
    assert!(settling(9_000_002, false, true), "seen finishing");
    assert!(
        settling(9_000_002, false, true),
        "drawn again while it settles"
    );
    assert!(!settling(9_000_003, true, false));
    assert!(!settling(9_000_003, false, false), "stopped, not finished");
    std::thread::sleep(SETTLE + std::time::Duration::from_millis(50));
    assert!(!settling(9_000_002, false, true), "once it has faded");
}

/// A download run, finished and settled leaves no state of its bar behind, its glide's nor
/// its settle's (gaps row E49): the maps kept every download ever drawn.
#[test]
fn settled_bars_leave_no_state() {
    let id = 9_000_101;
    let _ = glided(id, 0.4, gpui::div());
    assert!(!settling(id, true, false));
    let _ = glided(id, 0.6, gpui::div());
    assert!(settling(id, false, true), "seen finishing");
    assert!(
        !GLIDES.with(|g| g.borrow().contains_key(&id)),
        "a glide kept once stopped"
    );
    std::thread::sleep(SETTLE + std::time::Duration::from_millis(50));
    assert!(!settling(id, false, true), "once it has faded");
    assert!(
        !SETTLING.with(|s| s.borrow().contains_key(&id)),
        "its settle kept"
    );
    // Neither running nor finished: nothing kept either.
    assert!(!settling(id + 1, false, false));
    assert!(!SETTLING.with(|s| s.borrow().contains_key(&(id + 1))));
}

/// A gliding bar is drawn where it stands at the moment, a few frames a tick and no more: not
/// at the display's every refresh, which drew the whole window 120 times a second for as long
/// as a download ran (gaps row C0c).
#[test]
fn a_glide_is_drawn_a_few_times_a_tick_not_at_every_refresh() {
    let frames = GLIDE.as_secs_f32() / GLIDE_FRAME.as_secs_f32();
    assert!(
        (5.0..=10.0).contains(&frames),
        "{frames} frames a glide: too few read as jumps, more cost the window's CPU"
    );
    // Mid-glide, the bar is drawn on its way, and its glide is kept.
    let id = 7_000_101;
    let _ = glided(id, 0.1, gpui::div());
    let _ = glided(id, 0.9, gpui::div());
    std::thread::sleep(GLIDE / 2);
    let _ = glided(id, 0.9, gpui::div());
    let (from, to, set_out) = GLIDES.with(|g| g.borrow()[&id]);
    let here = glide_at(from, to, set_out.elapsed());
    assert!(here > 0.1 && here <= 0.9, "{here}");
    std::thread::sleep(GLIDE);
    assert_eq!(
        glide_at(from, to, set_out.elapsed()),
        0.9,
        "done within its tick"
    );
}
