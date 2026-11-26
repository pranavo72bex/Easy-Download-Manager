// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The clipboard: links picked from it, its watcher and its bar.

use super::*;

#[test]
fn links_on_the_clipboard_are_picked_and_put_in_the_main_queue() {
    let world = world();
    let mut h = world.harness();
    let text = format!(
        "see {} and {} and not this",
        world.url("file.zip"),
        world.url("clip.bin")
    );
    clip(&mut h, &text);
    h.command("program.add_from_clipboard");
    h.until("Links on the clipboard", |h| {
        h.dialog_open("Links on the clipboard")
    });
    assert!(h.shows("Found 2 links; 2 ticked."), "{:?}", h.texts());
    assert!(h.shows_part("file.zip") && h.shows_part("clip.bin"));
    h.click("Add for later");
    h.until("both rows", |h| h.rows().len() == 2);
    assert!(h
        .rows()
        .iter()
        .all(|r| r.queue == dm_core::MAIN_QUEUE && r.state == State::Paused));
}

/// Add links from clipboard offers an address already in the list unticked and says so.
#[test]
fn a_clipboard_link_already_listed_is_offered_unticked() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let text = format!("{} {}", world.url("file.zip"), world.url("other.pdf"));
    clip(&mut h, &text);
    h.command("program.add_from_clipboard");
    h.until("Links on the clipboard", |h| {
        h.dialog_open("Links on the clipboard")
    });
    assert!(h.shows("Found 2 links; 1 ticked."), "{:?}", h.texts());
    assert!(h.shows_part("(already in the list)"));
    // Only the new one is added.
    h.click("Add for later");
    h.until("the new row", |h| h.rows().len() == 2);
    assert_eq!(
        h.rows()
            .iter()
            .filter(|r| r.url == world.url("file.zip"))
            .count(),
        1
    );
}

#[test]
fn the_clipboard_watcher_offers_only_new_links_and_only_when_on() {
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some(world.url("old.iso"));
    let mut h = world.harness();
    let offer = "Link on your clipboard:";
    // Each look is a second apart: two wakes a second apart look twice.
    let looked = |h: &mut Harness| {
        let at = std::time::Instant::now();
        h.until("a look at the clipboard", |_| {
            at.elapsed() > crate::platform::clipwatch::EVERY * 2
        });
    };
    clip(&mut h, &world.url("new.iso"));
    looked(&mut h);
    assert!(!h.shows(offer), "offered while off");

    // By its command: on macOS a new dm has no toolbar Options (the app menu's Settings…).
    h.command("downloads.options");
    h.until("Options", |h| h.dialog_open("Options"));
    let watch = "Offer to download links I copy";
    // Lower on General than the window shows: scrolled to, as a person scrolls, as far as it
    // takes (Windows' General has more above it).
    h.reveal(watch);
    h.click(watch);
    h.click("OK");
    h.until("the watch on", |h| {
        h.view(|v| v.backend.settings().general.watch_clipboard)
    });
    looked(&mut h);
    assert!(!h.shows(offer), "offered what was there before");

    clip(&mut h, "just some words");
    looked(&mut h);
    assert!(!h.shows(offer));
    let fresh = world.url("fresh.iso");
    clip(&mut h, &fresh);
    h.until_shown(offer);
    assert!(h.shows_part("fresh.iso"));
    h.click("Dismiss");
    clip(&mut h, "elsewhere");
    looked(&mut h);
    clip(&mut h, &fresh);
    looked(&mut h);
    assert!(!h.shows(offer), "a dismissed link came back");
}

/// Links on the clipboard can be started at once, or not at all.
#[test]
fn links_on_the_clipboard_can_start_now_or_be_cancelled() {
    let world = world();
    let mut h = world.harness();
    clip(&mut h, &world.url("file.zip"));
    h.command("program.add_from_clipboard");
    h.until("Links on the clipboard", |h| {
        h.dialog_open("Links on the clipboard")
    });
    h.click("Cancel");
    h.until("it to close", |h| h.dialogs().is_empty());
    assert!(h.rows().is_empty());
    h.command("program.add_from_clipboard");
    h.until("Links on the clipboard", |h| {
        h.dialog_open("Links on the clipboard")
    });
    h.click("Start now");
    h.until("it to finish", finished);
}

/// Several links copied at once are all offered, through the dialog that lists them; only the
/// first was, and the rest dropped.
#[test]
fn the_clipboard_watcher_offers_every_link_copied_together() {
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some("old".into());
    let mut h = world.harness();
    h.settings_to(|s| s.general.watch_clipboard = true);
    let links = [
        world.url("file.zip"),
        world.url("other.pdf"),
        world.url("deep.pdf"),
    ];
    clip(&mut h, &links.join("\n"));
    h.until_shown("Links on your clipboard: 3");
    h.click("Download…");
    h.until("Links on the clipboard", |h| {
        h.dialog_open("Links on the clipboard")
    });
    assert!(h.shows("Found 3 links; 3 ticked."), "{:?}", h.texts());
    for name in ["file.zip", "other.pdf", "deep.pdf"] {
        assert!(h.shows_part(name), "{name} is not offered");
    }
}

/// A long link on the clipboard is cut short, and its Download and Dismiss stay in the window.
#[test]
fn the_clipboard_bar_keeps_its_buttons_in_the_window() {
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some("old".into());
    let mut h = world.harness();
    h.settings_to(|s| s.general.watch_clipboard = true);
    let long = world.url(&format!("{}.zip", "a-very-long-name".repeat(8)));
    clip(&mut h, &long);
    h.until_shown("Dismiss");
    let windows = h.call("ui.windows", serde_json::Value::Null).unwrap();
    let main = windows["main"].as_array().unwrap();
    let width = main[2].as_f64().unwrap() - main[0].as_f64().unwrap();
    let texts = h.call("ui.texts", serde_json::Value::Null).unwrap();
    for name in ["Download", "Dismiss"] {
        let right = texts["texts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["text"] == name)
            .map(|t| t["rect"][2].as_f64().unwrap())
            .unwrap_or_else(|| panic!("no {name}"));
        assert!(right <= width, "{name} ends at {right}, past {width}");
    }
}

/// At a phone's width the link copied is shown, with Download and Dismiss on a line of their
/// own under it (on one line it was cut to "…"); at a desktop's width all is on one line.
#[test]
fn at_a_phones_width_the_clipboard_bar_shows_the_link_with_its_buttons_under_it() {
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some("old".into());
    let mut h = world.harness();
    h.settings_to(|s| s.general.watch_clipboard = true);
    let link = world.url("fresh.iso");
    clip(&mut h, &link);
    h.until_shown("Dismiss");
    let shown = dm_desktop_base::format::shown_url(&link);
    let one_line = |h: &mut Harness| {
        let label = h.bounds_of("Link on your clipboard:");
        let button = h.bounds_of("Dismiss");
        (
            button.origin.y < label.origin.y + label.size.height,
            label,
            button,
        )
    };
    let (together, ..) = one_line(&mut h);
    assert!(
        together,
        "a desktop's window put the buttons under the link"
    );
    h.resize(412.0, 915.0);
    h.until("the phone's layout", |h| !one_line(h).0);
    let (_, label, _) = one_line(&mut h);
    let link_at = h.bounds_of(&shown);
    assert!(
        f32::from(link_at.size.width) >= 90.0,
        "the link is {:?} wide: {:?}",
        link_at.size.width,
        h.texts()
    );
    assert!(link_at.origin.y < label.origin.y + label.size.height);
}

/// A video link copied at a moment (`&t=34`) offers that moment as the part's start in
/// Download details; one click ticks the part and fills it.
#[test]
fn a_link_copied_at_a_moment_offers_that_moment_as_the_part() {
    let world = world();
    let mut h = world.harness();
    details(
        &mut h,
        vec!["https://www.youtube.com/watch?v=jNQXAC9IVRw&t=34".into()],
    );
    h.until_shown("This link starts at 0:34.");
    h.click("From 0:34");
    h.until("the part filled in", |h| h.shows("0:34"));
    assert!(!h.shows("This link starts at 0:34."));
    h.click("Cancel");
}

/// With "Take a copied link at once" on, a link copied opens its Download details with no bar
/// first, as reference A's switch does; off, it is offered as before.
#[test]
fn a_copied_link_is_taken_at_once_where_that_is_on() {
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some("nothing yet".into());
    let mut h = world.harness();
    h.settings_to(|s| {
        s.general.watch_clipboard = true;
        s.start_copied = true;
    });
    let copied = world.url("taken.iso");
    clip(&mut h, &copied);
    h.until("Download details for it", |h| {
        h.dialog_open("Download details")
    });
    assert!(h.shows_part("taken.iso"));
    assert!(!h.shows("Link on your clipboard:"), "offered, not taken");
    h.close_dialogs();
    // Copied again, it is not taken twice.
    clip(&mut h, "other words");
    let at = std::time::Instant::now();
    h.until("a look", |_| {
        at.elapsed() > crate::platform::clipwatch::EVERY * 2
    });
    clip(&mut h, &copied);
    let at = std::time::Instant::now();
    h.until("a look", |_| {
        at.elapsed() > crate::platform::clipwatch::EVERY * 2
    });
    assert!(!h.dialog_open("Download details"), "taken twice");
    // Off again: offered in the bar, as before.
    h.settings_to(|s| s.start_copied = false);
    clip(&mut h, &world.url("offered.iso"));
    h.until_shown("Link on your clipboard:");
    assert!(h.shows_part("offered.iso"));
}

/// Another app's share of several links (a phone's) offers them to tick first, adding nothing
/// unasked; one shared link is added at once (gaps row H36).
#[test]
fn several_shared_links_are_offered_one_is_added() {
    let world = world();
    let mut h = world.harness();
    crate::platform::drops::SHARES_OFFERED.with(|s| s.set(true));
    let two = format!("{}\n{}", world.url("a.zip"), world.url("b.zip"));
    dm_desktop_base::drops::arrived(dm_desktop_base::drops::Dropped::Text(two));
    h.act(|view, cx| view.take_dropped_links(cx));
    h.until("Links shared with dm", |h| {
        h.dialog_open("Links shared with dm")
    });
    assert!(h.rows().is_empty(), "the shared links were added unasked");
    h.close_dialogs();
    dm_desktop_base::drops::arrived(dm_desktop_base::drops::Dropped::Text(world.url("c.zip")));
    h.act(|view, cx| view.take_dropped_links(cx));
    h.until("the one shared link added", |h| h.rows().len() == 1);
    crate::platform::drops::SHARES_OFFERED.with(|s| s.set(false));
}
