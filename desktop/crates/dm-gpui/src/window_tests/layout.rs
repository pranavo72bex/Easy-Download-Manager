// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where things are and that they can be reached: the columns, the windows a download opens,
//! dialogs at small sizes, the text size and the language, drags out of the list. Ported from
//! the egui window's `tests/window/layout.rs`; what was the egui window's own geometry
//! (its zoom past 120%, its scroll areas' frames, its painting) is not.

use std::time::{Duration, Instant};

use dm_core::State;
use dm_desktop_base::columns::Column;
use dm_shell::app::places::Place;
use gpui::{point, px, Bounds, Pixels};

use crate::harness::world::{world, World};
use crate::harness::{settings_file, Harness};

fn finished(h: &mut Harness) -> bool {
    h.rows().iter().any(|r| r.state == State::Completed)
}

/// `names` added for later, fetching nothing, and listed.
fn add_later(h: &mut Harness, world: &World, names: &[&str]) {
    let urls: Vec<String> = names.iter().map(|n| world.url(n)).collect();
    let before = h.rows().len();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": urls, "later": true, "again": true }),
    )
    .unwrap();
    h.until("the rows", |h| h.rows().len() == before + names.len());
}

/// The rows' names in the list's order.
fn shown(h: &mut Harness) -> Vec<String> {
    h.view(|view| {
        view.shown
            .iter()
            .filter_map(|&i| view.rows.get(i))
            .map(|r| dm_shell::app::names::name_of(r))
            .collect()
    })
}

/// Where the words `words` are in the main window, if drawn there.
fn in_main(h: &mut Harness, words: &str) -> Option<Bounds<Pixels>> {
    let texts = h.call("ui.texts", serde_json::Value::Null).unwrap();
    texts["texts"].as_array().unwrap().iter().find_map(|t| {
        (t["text"] == words && t["window"] == "main").then(|| {
            let r: Vec<f32> = t["rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect();
            Bounds::from_corners(point(px(r[0]), px(r[1])), point(px(r[2]), px(r[3])))
        })
    })
}

/// A window's frame on the screen.
fn frame(h: &mut Harness, window: gpui::AnyWindowHandle) -> Bounds<Pixels> {
    h.cx.update(|cx| window.update(cx, |_, window, _| window.bounds()).unwrap())
}

/// A window's inside: how much it shows.
fn inside(h: &mut Harness, window: gpui::AnyWindowHandle) -> gpui::Size<Pixels> {
    h.cx.update(|cx| {
        window
            .update(cx, |_, window, _| window.viewport_size())
            .unwrap()
    })
}

/// The window in front.
fn front(h: &mut Harness) -> gpui::AnyWindowHandle {
    h.cx.update(|cx| cx.active_window())
        .expect("a window in front")
}

/// The text size put in force for every window, as View › Text size puts it.
fn text_size(h: &mut Harness, size: f32) {
    h.act(|_, cx| crate::zoom::start(size, cx));
    h.act(|_, cx| cx.refresh_windows());
}

#[test]
fn a_finished_file_dragged_out_of_the_list_is_handed_to_the_systems_drag() {
    // What is measured here grows with the text size another test may set.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("stalled.bin"), "Add for later");
    h.until("the download to complete", finished);
    let done = h.row("file.zip");

    // Pressed on its name and moved: a drag, not a click. Held there, what the drag carries is
    // what the system is handed as it leaves the window.
    let drag = |h: &mut Harness, name: &str| {
        let at = h.bounds_of(name);
        let from = point(at.left() + px(8.0), at.center().y);
        let to = from + point(px(48.0), px(24.0));
        h.drag(from, to, false);
        let dragging = h.cx.update(|cx| cx.has_active_drag());
        h.release(to);
        dragging
    };
    assert!(
        drag(&mut h, "file.zip"),
        "the finished file was not dragged"
    );
    let carried = h.view(|view| crate::dragout::carried(view, done.id));
    assert_eq!(
        carried,
        [std::path::PathBuf::from(&done.path)],
        "the finished file was not carried"
    );
    // An unfinished row drags too, to take another's place in the queue (gaps row U39), but
    // carries no file out of the window.
    let waiting = h.row("stalled.bin");
    drag(&mut h, "stalled.bin");
    assert!(
        h.view(|view| crate::dragout::carried(view, waiting.id))
            .is_empty(),
        "an unfinished download has no file to carry"
    );
}

/// A column is widened by dragging its heading's right edge, as in any list, and kept at the
/// next launch.
#[test]
fn a_column_is_widened_by_dragging_its_edge() {
    // The width is kept in the settings file.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    add_later(&mut h, &world, &["file.zip"]);
    let width = |h: &mut Harness| h.view(|view| view.widths().get("size").copied());
    let before = width(&mut h);
    // Size's edge is just left of Status's heading, which starts where its column does (a
    // size stands at its column's right).
    let status = in_main(&mut h, "Status").expect("the Status heading");
    let mut moved = None;
    for back in 1..20 {
        let from = point(status.left() - px(back as f32), status.center().y);
        h.drag(from, from + point(px(80.0), px(0.0)), true);
        if width(&mut h) != before {
            moved = width(&mut h);
            break;
        }
    }
    let after = moved.expect("no drag just left of Status's heading widened Size");
    assert!(
        after > before.unwrap_or(72.0) + 40.0,
        "Size went from {before:?} to {after}"
    );

    // And kept at the next launch: a new window over the settings left.
    let settings = h.view(|view| view.backend.settings());
    drop(h);
    let mut again = world.harness_with(settings);
    add_later(&mut again, &world, &["file.zip"]);
    let relaunched = width(&mut again).unwrap_or_default();
    assert!(
        (relaunched - after).abs() < 1.0,
        "Size was {after} wide when left, {relaunched} at the next launch"
    );
}

/// The series' error is in the language in force, says nothing until From and To are typed,
/// and keeps its room so the buttons stay put.
#[test]
fn the_series_error_waits_for_both_ends_and_speaks_german() {
    use dm_desktop_base::i18n::t;
    // The language is the process's.
    let _file = settings_file();
    let world = world();
    let mut settings = world.settings();
    settings.language = "de".into();
    let mut h = world.harness_with(settings);
    dm_desktop_base::i18n::set("de");
    h.command("program.add_series");
    h.until("the series", |h| !h.dialogs().is_empty());
    let start = |h: &mut Harness| h.dialog_words(t("Start now")).expect("Start now");
    let before = start(&mut h);
    h.fill(t("Address with *"), &world.url("part.bin"));
    let star = t("The address needs exactly one * to fill in.");
    assert_ne!(star, "The address needs exactly one * to fill in.");
    assert!(!h.shows(star), "said before From/To");
    h.fill(t("From"), "5");
    assert!(!h.shows(star), "said before To");
    h.fill(t("To"), "1");
    h.until("the error, in German", |h| h.shows(star));
    assert_eq!(start(&mut h), before, "the dialog jumped");
    dm_desktop_base::i18n::set("en");
}

/// View › Text size makes everything larger, smaller, or normal again.
#[test]
fn view_text_size_zooms_the_window() {
    // The text size is the process's.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    for item in ["View", "Text size", "Larger"] {
        h.click(item);
    }
    h.until("120%", |_| (crate::zoom::factor() - 1.2).abs() < 0.01);
    for item in ["View", "Text size", "Normal size"] {
        h.click(item);
    }
    h.until("100%", |_| (crate::zoom::factor() - 1.0).abs() < 0.01);
}

#[test]
fn view_language_puts_the_window_in_another_language_and_back() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    assert!(h.shows("Transfer rate"));
    for item in ["View", "Language", "Deutsch"] {
        h.click(item);
    }
    h.until("German", |h| {
        h.view(|view| view.backend.settings().language) == "de"
    });
    h.until("the list's headings in German", |h| {
        h.shows("Übertragungsrate") && !h.shows("Transfer rate")
    });
    for item in ["Ansicht", "Sprache", "English"] {
        h.click(item);
    }
    h.until("not back in English", |h| h.shows("Transfer rate"));
    assert_eq!(h.view(|view| view.backend.settings().language), "en");
}

#[test]
fn five_hundred_rows_draw_inside_a_frame_budget() {
    // What is measured here grows with the text size another test may set.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    let links: Vec<String> = (0..500)
        .map(|i| world.url(&format!("stalled-{i}.bin")))
        .collect();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": links, "later": true }),
    )
    .unwrap();
    h.until("500 rows", |h| h.rows().len() == 500);

    // The quickest of thirty frames, every window drawn afresh: the suite's other tests and
    // whatever else the machine is doing stretch a frame, not the window's own work.
    let frame = |h: &mut Harness| {
        (0..30)
            .map(|_| {
                let started = Instant::now();
                h.cx.update(|cx| cx.refresh_windows());
                h.cx.run_until_parked();
                started.elapsed()
            })
            .min()
            .unwrap_or_default()
    };
    let per_frame = frame(&mut h);
    println!("500 rows: {per_frame:?} a frame");
    // Windows' runners draw without a GPU (WARP, the system's software adapter): 87 to 125 ms
    // there, where a computer with a GPU draws in a fraction of it. Linux's runners too (Mesa's
    // software Vulkan): 104 to 167 ms (2026-11-13), where a Mac takes 44.
    let budget = if cfg!(windows) {
        160
    } else if cfg!(target_os = "linux") {
        200
    } else {
        100
    };
    assert!(
        per_frame < Duration::from_millis(budget),
        "{per_frame:?} a frame"
    );
    // With the search box's words, too.
    h.command("downloads.find");
    h.type_text("stalled-4");
    h.until("the box to narrow", |h| {
        h.view(|view| view.shown.len()) < 500
    });
    let filtered = frame(&mut h);
    println!("500 rows, narrowed: {filtered:?} a frame");
    assert!(
        filtered < Duration::from_millis(100),
        "{filtered:?} a frame"
    );
}

/// Properties fit what they hold in their own window, OK reached, and the window's close
/// closes them.
#[test]
fn properties_fit_a_small_window_and_close_from_the_title_bar() {
    // What is measured here grows with the text size another test may set.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.resize(720.0, 460.0);
    let long = format!("https://example.com/{}", "very-long-page-name/".repeat(30));
    let url = world.url("file.zip");
    h.view(|view| {
        view.backend.add_with(
            &url,
            dm_core::AddOptions {
                referrer: long,
                later: true,
                ..Default::default()
            },
        )
    })
    .unwrap();
    h.until("the row", |h| h.rows().len() == 1);
    h.select_by_port("file.zip");
    h.command("file.properties");
    h.until("Properties", |h| !h.dialogs().is_empty());
    let window = front(&mut h);
    h.reveal("OK");
    let ok = h.dialog_words("OK").expect("OK");
    let size = inside(&mut h, window);
    assert!(
        ok.right() <= size.width && ok.bottom() <= size.height,
        "OK is off its window: {ok:?} in {size:?}"
    );
    // Its close button's way, as File › Close (⌘W) takes it.
    h.key("w", &["cmd"]);
    h.until("Properties to close", |h| {
        h.update(|view, cx| view.properties_open(cx)) == 0
    });
}

/// A long refusal wraps in the progress window, which keeps Retry in sight; the status bar
/// cuts a long sentence rather than running past the window.
#[test]
fn long_status_text_wraps_and_stays_on_screen() {
    // What is measured here grows with the text size another test may set.
    let _file = settings_file();
    let world = world();
    let mut settings = world.settings();
    settings.show_progress = true;
    let mut h = world.harness_with(settings);
    h.resize(720.0, 600.0);
    let long = format!("{}secret.zip", "a-long-path/".repeat(12));
    h.add_url(&world.url(&long), "Start now");
    h.until("the refusal", |h| {
        h.rows().iter().any(|r| matches!(r.state, State::Failed(_)))
    });
    // The server's answer, a question of its own in front, put away.
    h.until("the question", |h| h.shows("The file doesn't exist"));
    h.click("OK");
    h.until("the progress window's Retry", |h| {
        h.dialog_words("Retry").is_some()
    });
    let window = front(&mut h);
    let size = inside(&mut h, window);
    let retry = h.dialog_words("Retry").unwrap();
    assert!(
        retry.right() <= size.width && retry.bottom() <= size.height,
        "{retry:?} in {size:?}"
    );
    h.close_dialogs();
    h.view(|view| {
        view.said = Some((
            format!(
                "this computer could not open {}: xdg-open: {}",
                "/x".repeat(30),
                "lynx: not found ".repeat(10)
            ),
            Instant::now(),
        ))
    });
    h.until("the sentence", |h| {
        h.shows_part("this computer could not open")
    });
    let texts = h.call("ui.texts", serde_json::Value::Null).unwrap();
    for t in texts["texts"].as_array().unwrap() {
        if t["window"] == "main"
            && t["text"]
                .as_str()
                .is_some_and(|s| s.contains("this computer could not open"))
        {
            let right = t["rect"][2].as_f64().unwrap();
            assert!(right <= 720.5, "the status bar ran off the window: {t}");
        }
    }
}

/// Add URL's warning is said and OK stays where it was, under the pointer (gaps row U44), and
/// Download details shows the size once the server says it.
#[test]
fn dialogs_keep_their_place_and_show_the_size() {
    // Add URL saves its addresses.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.click("Add URL");
    h.until_shown("Address");
    let two = format!("{}{}", world.url("a.zip"), world.url("b.zip"));
    h.type_text(&two);
    let ok = h.bounds_of("OK");
    h.click("OK");
    h.until("the warning", |h| h.shows_part("two links"));
    assert_eq!(h.bounds_of("OK"), ok, "OK moved as the warning appeared");
    h.close_dialogs();
    let url = world.url("file.zip");
    h.act(|_, cx| crate::details::open(cx.entity().downgrade(), vec![url], cx));
    h.until("the size in Download details", |h| {
        h.shows_part(&dm_desktop_base::format::bytes(256 * 1024))
    });
}

/// The columns follow the window's width until one is dragged: kept from the first draw, a
/// list opened narrow and widened left the name at 120 points beside empty room.
#[test]
fn the_columns_follow_the_window_until_one_is_dragged() {
    // What is measured here grows with the text size another test may set.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.resize(720.0, 460.0);
    add_later(&mut h, &world, &["file.zip"]);
    let name_width = |h: &mut Harness| {
        let name = in_main(h, "File Name").expect("File Name").left();
        let status = in_main(h, "Status").expect("Status").left();
        status - name
    };
    let narrow = name_width(&mut h);
    h.resize(1600.0, 800.0);
    let wide = name_width(&mut h);
    assert!(
        wide > narrow + px(40.0),
        "the name stayed {narrow:?} wide, now {wide:?}"
    );
}

/// A progress window opens below the toolbar, not over Add URL, and each cascades from the
/// one before; Download complete waits while the Exit question is asked, rather than opening
/// over it with Open folder under the pointer.
#[test]
fn windows_a_download_opens_keep_clear_of_the_toolbar_and_questions() {
    // What is measured here grows with the text size another test may set.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.resize(960.0, 660.0);
    for _ in 0..2 {
        let url = world.url("stalled.bin");
        h.view(|view| view.backend.add(&url)).unwrap();
    }
    h.until("both to run", |h| {
        h.rows()
            .iter()
            .filter(|r| r.state == State::Running)
            .count()
            == 2
    });
    let ids: Vec<_> = h.rows().iter().map(|r| r.id).collect();
    for id in &ids {
        h.update(|view, cx| view.show_progress(*id, cx));
    }
    h.until("both progress windows", |h| h.dialogs().len() == 2);
    let main = h.window;
    let main_frame = frame(&mut h, main);
    let add = h.bounds_of("Add URL");
    let windows: Vec<Bounds<Pixels>> = h
        .view(|view| view.progress_windows.values().copied().collect::<Vec<_>>())
        .into_iter()
        .map(|w| frame(&mut h, w))
        .collect();
    assert_eq!(windows.len(), 2);
    for window in &windows {
        assert!(
            window.top() > main_frame.top() + add.bottom(),
            "{window:?} over Add URL at {add:?} in {main_frame:?}"
        );
    }
    assert!(
        (windows[0].top() - windows[1].top()).abs() > px(10.0),
        "they do not cascade: {windows:?}"
    );

    h.close_dialogs();
    h.apply(|s| s.show_complete = true);
    h.click("Tasks");
    h.click("Exit");
    h.until("the Exit question", |h| h.view(|view| view.exit_asked));
    let url = world.url("file.zip");
    h.view(|view| view.backend.add(&url)).unwrap();
    h.until("it to finish", finished);
    assert!(
        !h.dialog_open("Download complete"),
        "opened over the Exit question"
    );
    h.click("Keep downloading");
    h.until("Download complete", |h| h.dialog_open("Download complete"));
}

/// The columns start where they did at the last launch whether or not a row has a queue yet;
/// Queue started narrower in an empty list, so a relaunch moved Size and the rest.
#[test]
fn the_columns_start_in_one_place_whatever_the_rows_queues() {
    // What is measured here grows with the text size another test may set.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    let before = in_main(&mut h, "Size").expect("Size").left();
    add_later(&mut h, &world, &["other.pdf"]);
    assert!(h.rows().iter().any(|r| !r.queue.is_empty()));
    let with_row = in_main(&mut h, "Size").expect("Size").left();
    assert!(
        (before - with_row).abs() < px(1.0),
        "Size moved from {before:?} to {with_row:?} as a queued row came"
    );
    // A launch: nothing of the list's widths remembered, no column dragged.
    let settings = h.view(|view| view.backend.settings());
    drop(h);
    let mut again = world.harness_with(settings);
    add_later(&mut again, &world, &["other.pdf"]);
    let after = in_main(&mut again, "Size").expect("Size").left();
    assert!(
        (before - after).abs() < px(1.0),
        "Size moved from {before:?} to {after:?}"
    );
}

/// At the largest text in the smallest window the places are still reached: the side panel
/// stays, where the egui window's gave way to a drop-down above the list.
#[test]
fn the_places_are_reachable_when_they_give_way_at_large_text() {
    // The text size is the process's.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.resize(720.0, 460.0);
    text_size(&mut h, 1.2);
    h.click("Videos");
    h.until("Videos chosen", |h| {
        h.view(|view| view.place == Place::Kind(dm_core::Category::Videos))
    });
    text_size(&mut h, 1.0);
}

/// The last row reached at large text: End (and the clipboard's bar above the list) leaves it
/// whole in the list, not under its edge.
#[test]
fn the_last_row_can_be_reached_at_large_text() {
    // The text size is the process's.
    let _file = settings_file();
    for clipboard_bar in [false, true] {
        let world = world();
        let mut h = world.harness();
        h.resize(720.0, 460.0);
        text_size(&mut h, 1.2);
        let names: Vec<String> = (0..34).map(|i| format!("stalled-{i:02}.bin")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        add_later(&mut h, &world, &names);
        if clipboard_bar {
            let offered = world.url("offered.zip");
            h.view(|view| view.clipboard.offered = Some(offered));
        }
        h.key("end", &[]);
        let last = shown(&mut h).last().unwrap().clone();
        h.until_shown(&last);
        let row = h.bounds_of(&last);
        let list = h.view(|view| view.scroll.0.borrow().base_handle.bounds());
        assert!(
            row.bottom() <= list.bottom() + px(0.5) && row.top() >= list.top(),
            "{last} at {row:?} outside the list {list:?}"
        );
        let main = h.window;
        let size = inside(&mut h, main);
        assert!(row.bottom() <= size.height, "{last} below the window");
        text_size(&mut h, 1.0);
    }
}

/// At the largest text in the smallest window a row still shows.
#[test]
fn a_row_shows_at_230_percent_in_the_smallest_window() {
    // The text size is the process's.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.resize(720.0, 460.0);
    add_later(&mut h, &world, &["other.pdf"]);
    text_size(&mut h, 1.2);
    let name = shown(&mut h)[0].clone();
    h.until_shown(&name);
    let row = h.bounds_of(&name);
    text_size(&mut h, 1.0);
    assert!(row.bottom() <= px(460.0), "{row:?} below the window");
}

/// The progress window was once held at the room on the screen, 84% of the window's height for
/// three connections. It is as tall as what it holds, its connections shut until asked for.
#[test]
fn the_progress_window_is_as_tall_as_what_it_holds() {
    // What is measured here grows with the text size another test may set.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    let url = world.url("stalled.bin");
    h.view(|view| view.backend.add(&url)).unwrap();
    h.until("it to run", |h| {
        h.rows().iter().any(|r| r.state == State::Running)
    });
    let id = h.rows()[0].id;
    h.update(|view, cx| view.show_progress(id, cx));
    h.until("the progress window", |h| h.dialogs().len() == 1);
    let window = front(&mut h);
    let tall = inside(&mut h, window).height;
    let main = h.window;
    let room = inside(&mut h, main).height;
    assert!(
        tall < room * 0.55,
        "{tall:?} is more than half the main window's {room:?}"
    );
}

/// Properties open in the middle, as the other dialogs do: they opened in the corner.
#[test]
fn properties_open_in_the_middle() {
    // What is measured here grows with the text size another test may set.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    add_later(&mut h, &world, &["file.zip"]);
    h.select_by_port("file.zip");
    h.command("file.properties");
    h.until("Properties", |h| !h.dialogs().is_empty());
    let window = front(&mut h);
    let at = frame(&mut h, window);
    assert!(
        at.left() > px(150.0),
        "Properties opened at the left: {at:?}"
    );
    assert!(at.top() > px(100.0), "Properties opened at the top: {at:?}");
}

/// A list with little room, as the largest text in the smallest window with the clipboard's
/// bar above leaves, stays where it was scrolled: it was pulled back a little every frame.
#[test]
fn a_list_with_room_for_one_row_stays_where_it_was_scrolled() {
    // The text size is the process's.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.resize(720.0, 460.0);
    text_size(&mut h, 1.2);
    let names: Vec<String> = (0..34).map(|i| format!("stalled-{i:02}.bin")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    add_later(&mut h, &world, &names);
    let offered = world.url("offered.zip");
    h.view(|view| view.clipboard.offered = Some(offered));
    h.key("end", &[]);
    let last = shown(&mut h).last().unwrap().clone();
    h.until_shown(&last);
    let first = h.bounds_of(&last).top();
    for _ in 0..30 {
        h.act(|_, cx| cx.notify());
    }
    let later = h.bounds_of(&last).top();
    text_size(&mut h, 1.0);
    assert!(
        (first - later).abs() < px(0.5),
        "the list moved from {first:?} to {later:?} by itself"
    );
}

/// The queue column's heading is named "Queue" (it said "Q", which read as a search glass; the
/// maintainer, 2026-11-06), and a click on it sorts the list by queue.
#[test]
fn the_queue_columns_heading_is_named_and_sorts() {
    // A sort is saved.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    add_later(&mut h, &world, &["file.zip"]);
    assert!(!h.shows("Q"), "the heading still says Q");
    // Its cell is just left of Size's, whose words stand at its right: Size's cell is 72
    // points wide, the queue's 22.
    let size = in_main(&mut h, "Size").expect("Size");
    let at = point(
        size.right() + px(8.0) - px(72.0) - px(11.0),
        size.center().y,
    );
    h.drag(at, at, true);
    h.until("the sort by queue", |h| {
        matches!(h.view(|view| view.sort), Some((Column::Queue, _)))
    });
}

/// A heading sorts up, then down, then back to the order added; the sort and View's Categories
/// and Toolbar are kept, and a window started with what was kept shows the list as it was left
/// (gaps row U29).
#[test]
fn the_sort_and_what_is_shown_come_back_after_a_restart() {
    let world = world();
    let mut h = world.harness();
    let sort = |h: &mut Harness| h.view(|view| view.sort);
    h.click("Size");
    h.until("sorted up", |h| sort(h) == Some((Column::Size, true)));
    h.click("Size");
    h.until("sorted down", |h| sort(h) == Some((Column::Size, false)));
    h.run(dm_shell::app::commands::Command::ToggleCategories);
    h.run(dm_shell::app::commands::Command::ToggleToolbar);
    let kept = h.view(|view| view.backend.settings());
    assert_eq!(kept.sort_column, Column::Size.key());
    assert!(!kept.sort_ascending);
    assert!(
        !kept.show_categories && !kept.show_toolbar,
        "the toggles were not kept"
    );

    // Started again with what was kept.
    let mut again = world.harness_with(kept);
    again.view(|view| {
        assert_eq!(view.sort, Some((Column::Size, false)));
        assert!(!view.show_sidebar && !view.show_toolbar);
    });

    // A third click: the order added, kept so.
    h.click("Size");
    h.until("the order added", |h| sort(h).is_none());
    assert!(h
        .view(|view| view.backend.settings())
        .sort_column
        .is_empty());
}
