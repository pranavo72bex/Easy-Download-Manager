// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A part of a video: its shape, a GIF, and where it is asked for.

use super::*;

/// A part can be made vertical or square in Download details: the choice shows once Part is
/// ticked, and goes with the part to its row.
#[test]
fn a_part_can_be_made_vertical_or_square() {
    let world = world();
    let mut h = world.harness();
    details(
        &mut h,
        vec!["https://www.youtube.com/watch?v=jNQXAC9IVRw&t=4".into()],
    );
    h.until_shown("From 0:04");
    assert!(!h.shows("Vertical 9:16"));
    h.click("From 0:04");
    h.until_shown("Vertical 9:16");
    // The part's "to" (the foot's "to" its queue is a Queue line of the form since gaps E10).
    h.type_into_nth("to", 0, "0:09");
    h.click("Vertical 9:16");
    h.click("Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let row = h.rows().last().cloned().unwrap();
    assert_eq!(row.shape, Some(dm_core::shape::Shape::Vertical));
    assert!(row.span.is_some());
}

/// A part can be saved as a GIF where dm can make one: the choice shows with the part, and
/// goes with it to its row.
#[cfg(target_os = "macos")]
#[test]
fn a_part_can_be_saved_as_a_gif() {
    let world = world();
    let mut h = world.harness();
    details(
        &mut h,
        vec!["https://www.youtube.com/watch?v=jNQXAC9IVRw&t=4".into()],
    );
    h.until_shown("From 0:04");
    assert!(!h.shows("GIF"));
    h.click("From 0:04");
    h.until_shown("GIF");
    // The part's "to" (the foot's "to" its queue is a Queue line of the form since gaps E10).
    h.type_into_nth("to", 0, "0:09");
    h.click("GIF");
    h.click("Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let row = h.rows().last().cloned().unwrap();
    assert!(row.gif, "the row does not carry the GIF");
    assert!(row.span.is_some());
}

/// "Delete file after" is chosen as a download is saved, in Download details, and goes with it
/// to its row.
#[test]
fn delete_file_after_is_chosen_when_saving() {
    let world = world();
    let mut h = world.harness();
    details(&mut h, vec![world.url("file.zip")]);
    named(&mut h, "file.zip");
    h.click("Never");
    h.click("1 hour");
    h.until_shown("1 hour");
    h.click("Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    assert_eq!(h.rows()[0].delete_after, Some(3600));
}

/// The part the browser panel's "Part" line asked for: Download details opens with "Only a
/// part of the video" on and its ends filled in, and a part given only an end starts from the
/// beginning.
#[test]
fn a_part_asked_for_in_the_browsers_panel_fills_download_details() {
    let world = world();
    let mut h = world.harness();
    h.settings_to(|s| s.general.ask_panel_downloads = true);
    for (from, to, shown_from) in [("1:30", "2:00", "1:30"), ("", "45", "0:00")] {
        h.hand(Request {
            url: world.url("file.zip"),
            referrer: "https://video.example/watch".into(),
            ask: true,
            from: from.into(),
            to: to.into(),
            ..Default::default()
        });
        h.until("Download details", |h| h.dialog_open("Download details"));
        h.until_shown(shown_from);
        assert!(h.shows(to), "{:?}", h.texts());
        assert!(h.shows("Only a part of the video"));
        h.click("Cancel");
        // A second hand-off from the site cancelled asks whether to leave the site to the
        // browser: not this test's.
        h.until("it to close", |h| {
            h.dialogs().is_empty() || h.shows("Keep handing them over")
        });
        if h.shows("Keep handing them over") {
            h.click("Keep handing them over");
            h.until("the question to go", |h| !h.shows("Keep handing them over"));
        }
    }
    // No part asked for: the dialog is as it was, the part's ends not shown.
    h.hand(Request {
        url: world.url("file.zip"),
        ask: true,
        ..Default::default()
    });
    h.until("Download details", |h| h.dialog_open("Download details"));
    named(&mut h, "file.zip");
    assert!(!h.shows("Vertical 9:16"), "the part is on: {:?}", h.texts());
}

/// With Download details off for the panel's videos, a part typed on the "Part" line still
/// opens it, the part filled in, as contract row B59 says: added at once, the part was dropped
/// and the whole video fetched. With nothing typed the video is added at once, as the setting
/// says.
#[test]
fn a_part_asked_for_opens_download_details_where_it_is_off() {
    let world = world();
    let mut h = world.harness();
    let hand = |h: &mut Harness, from: &str, to: &str| {
        h.hand(Request {
            url: world.url("file.zip"),
            referrer: "https://video.example/watch".into(),
            ask: true,
            from: from.into(),
            to: to.into(),
            ..Default::default()
        });
    };
    hand(&mut h, "1:30", "2:00");
    h.until("Download details opened for the part", |h| {
        h.dialog_open("Download details")
    });
    h.until_shown("1:30");
    assert!(h.shows("2:00"));
    assert!(h.rows().is_empty(), "added without asking");
    h.click("Cancel");
    h.until("it to close", |h| h.dialogs().is_empty());
    hand(&mut h, "", "");
    h.until("the video added at once", |h| {
        h.rows().iter().any(|r| r.url.ends_with("/file.zip"))
    });
    assert!(
        !h.dialog_open("Download details"),
        "asked, with nothing typed"
    );
}
