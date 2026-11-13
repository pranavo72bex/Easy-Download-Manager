// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The keyboard and the right-click: the list's keys, Escape and Enter in the dialogs, the
//! side panel's and the rows' menus. Ported from the egui window's `tests/window/keyboard.rs`.

use dm_core::State;
use dm_shell::app::places::Place;
use dm_shell::app::power::WhenDone;
use dm_shell::host::Request;

use crate::harness::world::{world, World};
use crate::harness::{settings_file, Harness};

fn finished(h: &mut Harness) -> bool {
    h.rows().iter().any(|r| r.state == State::Completed)
}

/// `names` added for later, fetching nothing, and listed.
fn add_later(h: &mut Harness, world: &World, names: &[&str]) {
    let urls: Vec<String> = names.iter().map(|n| world.url(n)).collect();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": urls, "later": true }),
    )
    .unwrap();
    let count = h.rows().len();
    h.until("the rows", |h| h.rows().len() >= count.max(names.len()));
}

/// The names of the rows a command acts on, in the list's order.
fn picked(h: &mut Harness) -> Vec<String> {
    h.view(|view| {
        view.chosen()
            .into_iter()
            .map(dm_shell::app::names::name_of)
            .collect()
    })
}

/// The name of the row the keyboard is on.
fn cursor(h: &mut Harness) -> Option<String> {
    h.view(|view| {
        let id = view.cursor?;
        view.rows
            .iter()
            .find(|r| r.id == id)
            .map(|r| dm_shell::app::names::name_of(r))
    })
}

/// The words the main window's list shows, in its order.
fn shown(h: &mut Harness) -> Vec<String> {
    h.view(|view| {
        view.shown
            .iter()
            .filter_map(|&i| view.rows.get(i))
            .map(|r| dm_shell::app::names::name_of(r))
            .collect()
    })
}

/// The window in front.
fn front(h: &mut Harness) -> Option<gpui::AnyWindowHandle> {
    h.cx.update(|cx| cx.active_window())
}

#[test]
fn the_category_tree_has_right_click_menus() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", finished);

    // A queue: Start queue runs the synchronization check on it. The menu is in front of the
    // toolbar's Start Queue, so its item is the first of that name.
    h.right_click("Recheck queue");
    h.until("the place menu", |h| {
        h.view(|view| view.place_menu.is_some())
    });
    h.click("Start Queue");
    assert_eq!(
        h.view(|view| view.place.clone()),
        Place::Queue(dm_core::RECHECK_QUEUE.into()),
        "a right-click selects the place"
    );
    h.until("the check", |h| h.said().contains("Checked"));

    // Done: Clear finished, from the menu (the toolbar has one too).
    h.right_click("Finished");
    h.until("the place menu", |h| {
        h.view(|view| view.place_menu.is_some())
    });
    h.click("Delete Completed");
    h.until("the finished row gone", |h| h.rows().is_empty());

    // A kind: categories are edited in Options › Save to.
    h.right_click("Videos");
    h.until("the place menu", |h| {
        h.view(|view| view.place_menu.is_some())
    });
    h.click("Add or edit categories…");
    h.until("Options", |h| h.dialog_open("Options"));
    let tab = h.dialog::<crate::options::Options, _>(|o, _, _| o.tab);
    assert_eq!(tab, dm_desktop_base::options::Tab::SaveTo);
}

/// Typing in the search box narrows the list to the rows whose name, address or note hold
/// every word, whatever their case; emptied, it shows them all again.
#[test]
fn the_list_narrows_as_a_filter_is_typed() {
    let world = world();
    let mut h = world.harness();
    let names: Vec<String> = (1..=12).map(|n| format!("part{n}.bin")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    add_later(&mut h, &world, &names);
    assert_eq!(shown(&mut h).len(), 12);
    h.command("downloads.find");
    h.type_text("PART1");
    h.until("the list to narrow", |h| shown(h).len() == 4);
    let mut seen = shown(&mut h);
    seen.sort();
    assert_eq!(
        seen,
        ["part1.bin", "part10.bin", "part11.bin", "part12.bin"]
    );
    h.until_shown("4 shown");
    h.type_text(" nothing-like-it");
    h.until("nothing shown", |h| shown(h).is_empty());
    h.until_shown("No download here matches the filter.");
    // The box's clear button.
    h.click("✕");
    h.until("all twelve again", |h| shown(h).len() == 12);
}

/// Escape closes the dialog in front, and nothing behind it: Options, without what was typed
/// into it; Add URL over Download details, whose note stays. (The egui window's menus and
/// drop-downs were its own, drawn over the dialog; the GPUI window's are the system's, which
/// close themselves, and Find is the main window's search box, not a dialog.)
#[test]
fn escape_closes_the_menu_or_the_dialog_in_front_and_nothing_behind_it() {
    // Add URL saves its addresses.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.command("downloads.options");
    h.until("Options", |h| h.dialog_open("Options"));
    h.click("Advanced");
    // A proxy of its own, its kind picked from the pop-up, so that its host can be typed.
    h.reveal("Proxy type");
    h.click("The computer's proxy settings");
    h.click("HTTP");
    h.fill("Host", "proxy.example");
    h.key("escape", &[]);
    h.until("Options to close", |h| !h.dialog_open("Options"));
    assert!(
        h.view(|view| view.backend.settings().proxy.host.is_empty()),
        "Escape kept what was typed"
    );

    // A dialog over a dialog: Add URL over Download details. Escape closes Add URL; Download
    // details, and what was typed into it, stay.
    h.add_url_open(&world.url("file.zip"));
    h.fill("Note", "keep me");
    h.command("program.add_url");
    h.until("Add URL over it", |h| h.dialog_open("Add URL"));
    h.key("escape", &[]);
    h.until("Add URL to close", |h| !h.dialog_open("Add URL"));
    assert!(
        h.dialog_open("Download details"),
        "Download details was closed under Add URL"
    );
    assert!(h.shows("keep me"));
    h.key("escape", &[]);
    h.until("Download details to close", |h| {
        !h.dialog_open("Download details")
    });
}

/// A dialog opened from the keyboard gives the keyboard back to what had it when it closes.
/// The GPUI window's toolbar buttons are not tab stops (as a macOS toolbar's are not), so what
/// had the keyboard here is the search box: ⌘N opens Add URL from it, and once Escape closes
/// Add URL, what is typed goes on into the box.
#[test]
fn closing_a_dialog_gives_the_keyboard_back_to_its_opener() {
    let world = world();
    let mut h = world.harness();
    h.command("downloads.find");
    h.type_text("ab");
    h.key("n", &["cmd"]);
    h.until("Add URL", |h| h.dialog_open("Add URL"));
    h.key("escape", &[]);
    h.until("Add URL to close", |h| !h.dialog_open("Add URL"));
    h.type_text("c");
    h.until("the box to have the keyboard again", |h| {
        h.view(|view| view.filter_text.clone()) == "abc"
    });
}

/// Shift+F10 opens the menu of the row with the keyboard's place, as on any desktop, under the
/// row. (F10 opening the first menu of the menu bar is owed: gaps row U6.)
#[test]
fn menus_open_from_the_keyboard() {
    // What is measured here grows with the text size another test may set.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    add_later(&mut h, &world, &["file.zip"]);
    h.select("file.zip");
    h.key("f10", &["shift"]);
    h.until("the row menu", |h| h.view(|view| view.menu_open()));
    let props = h.bounds_of("Properties");
    let row = h.bounds_of("file.zip");
    // Under the row, not over the places beside the list.
    assert!(
        props.left() >= row.left() - gpui::px(30.0) && props.top() >= row.top(),
        "the menu at {props:?} is not under the row at {row:?}"
    );
}

#[test]
fn the_lists_keys_do_not_reach_it_from_behind_a_dialog() {
    // Options' OK saves the settings.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", finished);
    h.select("file.zip");
    h.command("downloads.options");
    h.until("Options", |h| h.dialog_open("Options"));
    h.key("delete", &[]);
    h.key("enter", &[]);
    assert_eq!(h.rows().len(), 1, "Delete behind Options removed it");
    assert!(
        !h.dialogs()
            .iter()
            .any(|d| d.contains("Download progress") || d.contains("% of") || d.contains("Remove")),
        "a key reached the list: {:?}",
        h.dialogs()
    );
    // With the dialog gone, Delete is the list's again.
    h.close_dialogs();
    h.key("delete", &[]);
    h.until("Delete to act on the list", |h| {
        h.rows().is_empty() || !h.dialogs().is_empty()
    });
}

/// F3 and Enter in the search box find the next row it shows, and the search goes on as more
/// is typed. Find is the search box in the GPUI window (gaps row U30), and its Return hands the
/// keyboard to the list, where the arrows go on from the row found (the egui Find dialog's
/// field kept it); Return there opened the found row's progress window as well.
#[test]
fn f3_finds_the_next_one_while_find_is_open() {
    let world = world();
    let mut h = world.harness();
    add_later(&mut h, &world, &["alpha.bin", "beta.bin", "alphabet.bin"]);
    h.command("downloads.find");
    h.type_text("alpha");
    h.until("the box to narrow", |h| shown(h).len() == 2);
    // From the box's own field.
    h.key("f3", &[]);
    assert_eq!(picked(&mut h), ["alpha.bin"], "F3 in the field");
    h.key("enter", &[]);
    assert_eq!(picked(&mut h), ["alphabet.bin"], "Enter in the field");
    assert!(h.dialogs().is_empty(), "Enter opened {:?}", h.dialogs());
    // The keyboard on the list: Up goes back from the row found.
    h.key("up", &[]);
    assert_eq!(
        picked(&mut h),
        ["alpha.bin"],
        "the list does not have the keyboard"
    );
    // Back in the box (Find selects its words), typing goes on after them.
    h.command("downloads.find");
    h.key("right", &[]);
    h.type_text("bet");
    h.until("the box to read alphabet", |h| {
        h.view(|view| view.filter_text.clone()) == "alphabet"
    });
    h.key("f3", &[]);
    assert_eq!(picked(&mut h), ["alphabet.bin"], "F3 with the box open");
}

/// A dialog opened to be typed in takes the keyboard at once, and Enter presses its default
/// button.
#[test]
fn a_dialog_is_typed_in_at_once_and_enter_presses_its_default_button() {
    // Add URL saves its addresses.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.key("n", &["cmd"]);
    h.until("Add URL", |h| h.dialog_open("Add URL"));
    // No click in the field first.
    h.type_text(&world.url("file.zip"));
    h.key("enter", &[]);
    h.until("Download details", |h| h.dialog_open("Download details"));
    assert!(!h.dialog_open("Add URL"), "Enter did not press OK");
    h.until("its address", |h| h.shows_part("file.zip"));
    h.close_dialogs();

    h.key("f", &["cmd"]);
    h.type_text("file");
    h.until("the search box's words", |h| {
        h.view(|view| view.filter_text.clone()) == "file"
    });

    h.command("program.add_series");
    h.until("Add numbered series", |h| {
        h.dialog_open("Add numbered series")
    });
    h.type_text(&world.url("img*.jpg"));
    h.until("the series' address field", |h| {
        h.shows(&world.url("img*.jpg"))
    });
}

/// At the narrowest window and in German, and at the largest text, every toolbar button stays
/// in the window and clear of the others. (The egui window's classic look and its 200% zoom
/// are not the GPUI window's: its text goes to 120%.)
#[test]
fn the_toolbar_keeps_its_names_narrow_and_in_german() {
    // The language and the text size are the process's.
    let _file = settings_file();
    for size in [1.0, 1.2] {
        let world = world();
        let mut settings = world.settings();
        settings.language = "de".into();
        settings.text_size = size;
        let mut h = world.harness_with(settings);
        dm_desktop_base::i18n::set("de");
        h.act(|_, cx| crate::zoom::start(size, cx));
        h.resize(720.0, 460.0);
        let mut seen: Vec<(String, gpui::Bounds<gpui::Pixels>)> = Vec::new();
        for (_, label) in dm_shell::app::commands::toolbar_in(&[]) {
            let name = dm_desktop_base::i18n::t(label);
            if !h.shows(name) {
                // Shown as its picture alone where the names do not fit.
                continue;
            }
            let at = h.bounds_of(name);
            assert!(
                at.right() <= gpui::px(720.5),
                "{size}x: {name} is past the window's edge: {at:?}"
            );
            for (other, was) in &seen {
                assert!(!at.intersects(was), "{size}x: {name} runs into {other}");
            }
            seen.push((name.to_string(), at));
        }
        assert!(!seen.is_empty(), "no toolbar names: {:?}", h.texts());
        h.act(|_, cx| crate::zoom::start(1.0, cx));
    }
    dm_desktop_base::i18n::set("en");
}

/// The arrow keys, Home and End move the list's selection, Shift extends it, and the
/// keyboard's place goes with them.
#[test]
fn the_arrow_keys_move_the_selection_and_the_focus_shows() {
    let world = world();
    let mut h = world.harness();
    add_later(&mut h, &world, &["a.bin", "b.bin", "c.bin"]);
    h.key("down", &[]);
    assert_eq!(picked(&mut h), ["a.bin"]);
    assert_eq!(
        cursor(&mut h).as_deref(),
        Some("a.bin"),
        "no place where the keys are"
    );
    h.key("down", &[]);
    h.key("down", &["shift"]);
    assert_eq!(picked(&mut h), ["b.bin", "c.bin"]);
    h.key("home", &[]);
    assert_eq!(picked(&mut h), ["a.bin"]);
    h.key("end", &[]);
    assert_eq!(picked(&mut h), ["c.bin"]);
    assert_eq!(cursor(&mut h).as_deref(), Some("c.bin"));
    // With the keyboard's focus moved on by Tab, not into a text field, the list's keys still
    // work: Delete asks about the selected row.
    h.select("a.bin");
    h.key("tab", &[]);
    h.key("delete", &[]);
    h.until("the question", |h| {
        h.dialog_open("Remove an unfinished download")
    });
}

/// "Download complete" reaches every file (its list scrolls), cuts a long name so Close stays
/// in its window, and Escape closes all of it.
#[test]
fn the_complete_window_scrolls_cuts_long_names_and_closes_on_escape() {
    // What is measured here grows with the text size another test may set.
    let _file = settings_file();
    let world = world();
    let mut settings = world.settings();
    settings.show_complete = true;
    let mut h = world.harness_with(settings);
    let long = format!("{}.zip", "very-long-name-".repeat(20));
    let url = world.url("file.zip");
    h.view(|view| {
        view.backend.add_with(
            &url,
            dm_core::AddOptions {
                save_name: Some(long.clone()),
                ..Default::default()
            },
        )
    })
    .unwrap();
    let urls: Vec<String> = (0..11).map(|_| world.url("file.zip")).collect();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": urls, "again": true }),
    )
    .unwrap();
    h.until("all twelve to finish", |h| {
        let rows = h.rows();
        rows.len() == 12 && rows.iter().all(|r| r.state == State::Completed)
    });
    h.until("12 downloads finished.", |h| {
        h.shows("12 downloads finished.")
    });
    let window = front(&mut h).unwrap();
    let size = h.cx.update(|cx| {
        window
            .update(cx, |_, window, _| window.viewport_size())
            .unwrap()
    });
    let close = h.dialog_words("Close").expect("Close is drawn");
    assert!(
        close.right() <= size.width && close.bottom() <= size.height,
        "Close is off its window: {close:?} in {size:?}"
    );
    // The last file is reached by the wheel over the list.
    let folders = |h: &mut Harness| {
        let texts = h.call("ui.texts", serde_json::Value::Null).unwrap();
        texts["texts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["window"] == "dialog" && t["text"] == "Open folder")
            .count()
    };
    // Several at a time, not one (its own window shows three at its first size).
    assert!(
        folders(&mut h) >= 3,
        "only {} files in view",
        folders(&mut h)
    );
    // The first to finish is listed last: the wheel over the list brings it above Close, its
    // name cut to the window.
    let named = |h: &mut Harness| {
        let texts = h.call("ui.texts", serde_json::Value::Null).unwrap();
        texts["texts"].as_array().unwrap().iter().find_map(|t| {
            let starts = t["text"]
                .as_str()
                .is_some_and(|s| s.starts_with("very-long-name"));
            (t["window"] == "dialog" && starts).then(|| {
                let r = t["rect"].as_array().unwrap();
                [0, 1, 2, 3].map(|i| r[i].as_f64().unwrap() as f32)
            })
        })
    };
    for _ in 0..40 {
        if named(&mut h).is_some_and(|r| r[3] <= f32::from(close.top())) {
            break;
        }
        h.scroll(&dm_desktop_base::format::bytes(256 * 1024), 3.0);
    }
    let at = named(&mut h).expect("the long name never came in sight");
    assert!(
        at[3] <= f32::from(close.top()),
        "the last file is not reachable: {at:?} under {close:?}"
    );
    h.key("escape", &[]);
    h.until("the window to close", |h| {
        !h.dialog_open("Download complete") && h.dialogs().is_empty()
    });
}

/// Tab goes round a dialog's own controls, never to the window behind it, and Enter with none
/// of them focused presses the default.
#[test]
fn tab_stays_in_the_dialog_and_enter_presses_its_default() {
    let world = world();
    let mut h = world.harness();
    let url = world.url("file.zip");
    h.act(|_, cx| crate::details::open(cx.entity().downgrade(), vec![url], cx));
    h.until("Download details", |h| h.dialog_open("Download details"));
    let dialog = front(&mut h).unwrap();
    for _ in 0..12 {
        h.key("tab", &[]);
        assert_eq!(front(&mut h), Some(dialog), "Tab left the dialog");
    }
    assert!(!h.view(|view| view.menu_open()), "Tab opened a menu behind");
    // Nothing in it focused but the dialog itself: Enter is Start now.
    h.dialog::<crate::details::Details, _>(|d, window, cx| window.focus(&d.focus, cx));
    h.key("enter", &[]);
    h.until("Start now", |h| !h.dialog_open("Download details"));
    assert_eq!(h.rows().len(), 1);
}

/// The Delete key asks as the toolbar's Delete does, for a finished row neither (it leaves
/// the list without a question, its file stays, with Undo a while); the speed limiter's field
/// has the keyboard and Enter applies it.
#[test]
fn the_delete_key_asks_and_the_limiter_takes_enter() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("it to finish", finished);
    h.select("file.zip");
    h.key("delete", &[]);
    h.until("the row gone", |h| h.rows().is_empty());
    assert!(h.dialogs().is_empty(), "Delete asked first");
    h.click("Undo");
    h.until("Undo to put it back", |h| h.rows().len() == 1);
    h.select("file.zip");
    h.click("Delete");
    h.until("the toolbar's Delete", |h| h.rows().is_empty());
    assert!(h.dialogs().is_empty(), "the toolbar's Delete asked");

    h.command("downloads.speed_limiter.settings");
    h.until("the limiter", |h| !h.dialogs().is_empty());
    h.key("a", &["cmd"]);
    h.type_text("321");
    h.key("enter", &[]);
    h.until("Enter to apply", |h| h.dialogs().is_empty());
    assert_eq!(h.view(|view| view.backend.settings().speed_limit), 321);
    h.apply(|s| s.speed_limit = 0);
}

/// A right click moves the keyboard's place to the row clicked.
#[test]
fn a_right_click_moves_the_keyboards_place() {
    let world = world();
    let mut h = world.harness();
    add_later(&mut h, &world, &["one.zip", "two.zip", "three.zip"]);
    h.key("home", &[]);
    let first = cursor(&mut h);
    h.right_click("three.zip");
    assert_ne!(first.as_deref(), Some("three.zip"));
    assert_eq!(
        cursor(&mut h).as_deref(),
        Some("three.zip"),
        "the cursor stayed behind"
    );
    let three = h.row("three.zip").id;
    assert_eq!(
        h.view(|view| view.anchor),
        Some(three),
        "Shift would extend from elsewhere"
    );
}

/// Enter presses Start now in "Links on the clipboard" and in "Download links".
#[test]
fn enter_starts_the_links_dialogs() {
    let world = world();
    let mut h = world.harness();
    h.cx.write_to_clipboard(gpui::ClipboardItem::new_string(world.url("file.zip")));
    h.command("program.add_from_clipboard");
    h.until("Links on the clipboard", |h| {
        h.dialog_open("Links on the clipboard")
    });
    h.key("enter", &[]);
    h.until("Enter to start it", |h| {
        !h.dialog_open("Links on the clipboard")
    });
    h.until("the row", |h| h.rows().len() == 1);
    h.settings_to(|s| s.ask_panel_downloads = true);
    h.hand(Request {
        url: world.url("page.html"),
        referrer: world.url("page.html"),
        links: vec![world.url("other.pdf")],
        ask: true,
        ..Default::default()
    });
    h.until("Download links", |h| h.dialog_open("Download links"));
    h.key("enter", &[]);
    h.until("Enter to start it", |h| !h.dialog_open("Download links"));
    h.until("both rows", |h| h.rows().len() == 2);
}

/// Page Down and Page Up move a screenful; End brings the last row wholly into view.
#[test]
fn page_keys_move_a_page_and_end_shows_the_last_row_whole() {
    // What is measured here grows with the text size another test may set.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.resize(720.0, 460.0);
    let names: Vec<String> = (0..40).map(|i| format!("stalled-{i:02}.bin")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    add_later(&mut h, &world, &names);
    let at = |h: &mut Harness| h.view(|view| view.cursor.and_then(|c| view.position_of(c)));
    h.key("home", &[]);
    assert_eq!(at(&mut h), Some(0));
    let page = h.view(|view| view.page());
    assert!(page > 2, "a page of {page} rows");
    h.key("pagedown", &[]);
    assert_eq!(at(&mut h), Some(page), "Page Down did not move a page");
    h.key("pageup", &[]);
    assert_eq!(at(&mut h), Some(0), "Page Up did not move back");

    h.key("end", &[]);
    assert_eq!(at(&mut h), Some(39));
    let last = shown(&mut h)[39].clone();
    h.until_shown(&last);
    let row = h.bounds_of(&last);
    let list = h.view(|view| view.scroll.0.borrow().base_handle.bounds());
    assert!(
        row.bottom() <= list.bottom() + gpui::px(0.5) && row.top() >= list.top(),
        "the last row {row:?} is not wholly in the list {list:?}"
    );
}

/// The Remove dialog's buttons agree with its sentence, Enter keeps, and after a removal the
/// next row is selected.
#[test]
fn remove_says_one_thing_enter_keeps_and_the_next_row_is_selected() {
    let world = world();
    let mut h = world.harness();
    add_later(
        &mut h,
        &world,
        &["stalled-a.bin", "stalled-b.bin", "stalled-c.bin"],
    );
    let ids: Vec<dm_core::Id> = h.view(|view| {
        view.shown
            .iter()
            .filter_map(|&i| view.rows.get(i))
            .map(|r| r.id)
            .collect()
    });
    h.select("stalled-b.bin");
    h.key("delete", &[]);
    h.until("the question", |h| h.shows("Keep it"));
    assert!(!h.shows("Keep them"));
    h.key("enter", &[]);
    h.until("the question to go", |h| h.dialogs().is_empty());
    assert_eq!(h.rows().len(), 3, "Enter deleted it");

    h.key("delete", &[]);
    h.until("the question", |h| h.shows("Yes, remove"));
    h.click("Yes, remove");
    h.until("the row gone", |h| h.rows().len() == 2);
    assert_eq!(
        h.view(|view| view.selected.clone()),
        [ids[2]].into(),
        "the selection was lost"
    );
}

/// Enter answers "Already in the list" with the answer that changes nothing — Show it — since
/// an Enter meant for the dialog before it fetched the file again.
#[test]
fn enter_shows_the_row_when_already_listed() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    let url = world.url("file.zip");
    h.add_url(&url, "Add for later");
    h.until("the row", |h| h.rows().len() == 1);
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(&url);
    h.click("OK");
    h.until("Already in the list", |h| h.shows("Already in the list"));
    h.key("enter", &[]);
    h.until("the question to go", |h| !h.shows("Already in the list"));
    assert!(!h.dialog_open("Download details"));
    assert_eq!(h.rows().len(), 1);
    assert_eq!(
        h.view(|view| view.selected.len()),
        1,
        "the row is not shown"
    );
}

/// Escape cancels the When-done countdown, even with Download complete open behind it, and
/// the keyboard starts on Cancel.
#[test]
fn escape_cancels_the_when_done_countdown() {
    let world = world();
    let mut settings = world.settings();
    settings.show_complete = true;
    let mut h = world.harness_with(settings);
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("it to finish", finished);
    h.until("Download complete", |h| h.dialog_open("Download complete"));
    h.view(|view| {
        view.session.countdown = Some((
            WhenDone::Sleep,
            std::time::Instant::now() + std::time::Duration::from_secs(30),
        ))
    });
    h.until("the countdown", |h| {
        h.shows_part("put the computer to sleep in")
    });
    h.key("escape", &[]);
    h.until("Escape to cancel it", |h| {
        h.view(|view| view.session.countdown.is_none())
    });
    assert!(
        !world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o.contains("power")),
        "it slept"
    );
}

/// Escape closes the window in front, not one behind it. Download complete and a progress
/// window stay open side by side; Escape closed Download complete whichever was in front.
#[test]
fn escape_closes_the_window_in_front() {
    let world = world();
    let mut h = world.harness();
    add_later(&mut h, &world, &["stalled.bin"]);
    let id = h.rows()[0].id;
    for progress_in_front in [true, false] {
        h.update(|view, cx| view.show_progress(id, cx));
        h.until("the progress window", |h| h.dialogs().len() == 1);
        let progress = front(&mut h).unwrap();
        let list = h.update(|view, cx| {
            view.complete.insert(id);
            cx.entity().downgrade()
        });
        let complete = h.cx.update(|cx| crate::complete::open(list, cx));
        h.view(|view| view.complete_window = complete);
        h.until("Download complete", |h| h.dialog_open("Download complete"));
        if progress_in_front {
            h.cx.update(|cx| {
                progress
                    .update(cx, |_, window, _| window.activate_window())
                    .unwrap()
            });
        }
        h.key("escape", &[]);
        h.until("one window to close", |h| h.dialogs().len() == 1);
        assert_eq!(
            h.dialog_open("Download complete"),
            progress_in_front,
            "the window behind closed"
        );
        h.close_dialogs();
        h.view(|view| view.complete.clear());
    }
}
