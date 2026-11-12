// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every dialog a window of its own: placed over the main window, its words a dialog's to a
//! script, closed by its close button; two Properties side by side, each with its own Cancel.
//! Ported from the egui window's `tests/dialog_windows.rs`.

use dm_shell::app::commands::Command;

use crate::harness::world::world;
use crate::harness::Harness;

/// The open windows other than the main one, with where each is.
fn dialog_windows(h: &mut Harness) -> Vec<(gpui::AnyWindowHandle, gpui::Bounds<gpui::Pixels>)> {
    let main = h.window;
    h.cx.update(|cx| {
        cx.windows()
            .into_iter()
            .filter(|w| *w != main)
            .filter_map(|w| w.update(cx, |_, window, _| (w, window.bounds())).ok())
            .collect()
    })
}

/// About opens as a window of its own over the main window; a script sees its words as a
/// dialog's; its close button closes it (B42).
#[test]
fn about_opens_in_a_window_of_its_own_over_the_main_window_and_closes_with_its_button() {
    let world = world();
    let mut h = world.harness();
    h.run(Command::About);
    h.until("About", |h| !h.dialogs().is_empty());
    let windows = h.call("ui.windows", serde_json::json!({})).unwrap();
    let title = windows["dialogs"][0]["title"].as_str().unwrap().to_string();
    assert!(title.contains("About"), "{windows}");
    h.until("About's words", |h| {
        h.shows_in_dialog("A free download manager, written in Rust.")
    });
    let main =
        h.cx.update(|cx| h.window.update(cx, |_, window, _| window.bounds()).unwrap());
    let about = dialog_windows(&mut h);
    assert_eq!(about.len(), 1);
    let (handle, at) = about[0];
    // Over the main window's middle.
    let (middle, centre) = (at.center(), main.center());
    assert!(
        (middle.x - centre.x).abs() < gpui::px(2.0),
        "{at:?} over {main:?}"
    );
    // Its close button, as the system closes a window.
    h.cx.update(|cx| {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    });
    h.until("About to close", |h| h.dialogs().is_empty());
}

/// Properties of two downloads open in two windows, not at one place; each Cancel is its own.
#[test]
fn properties_of_two_downloads_open_in_windows_of_their_own() {
    let world = world();
    let mut h = world.harness();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": [world.url("first.bin"), world.url("second.bin")],
                            "later": true }),
    )
    .unwrap();
    h.until("both rows", |h| h.rows().len() == 2);
    for name in ["first.bin", "second.bin"] {
        h.select(name);
        h.run(Command::Properties);
    }
    h.until("two Properties", |h| {
        h.dialogs()
            .iter()
            .filter(|d| *d == "File properties")
            .count()
            == 2
    });
    let places: Vec<_> = dialog_windows(&mut h)
        .into_iter()
        .map(|(_, at)| at.origin)
        .collect();
    assert_eq!(places.len(), 2);
    assert_ne!(places[0], places[1], "both Properties at one place");
    h.click("Cancel");
    h.until("one Properties left", |h| {
        h.dialogs()
            .iter()
            .filter(|d| *d == "File properties")
            .count()
            == 1
    });
}
