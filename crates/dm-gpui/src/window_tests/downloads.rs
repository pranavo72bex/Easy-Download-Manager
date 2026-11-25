// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Downloads from start to finish in the window: added, announced, opened, extracted,
//! previewed, renamed, removed, retried, and what each says on the way.

use std::time::Duration;

use dm_core::State;
use dm_shell::app::commands::Command;
use dm_shell::Settings;

use crate::harness::world::{payload, world, SECRET_ASKED};
use crate::harness::Harness;

mod failures;
mod finished_files;
mod hand_offs;
mod names_and_folders;
mod progress;

fn completed(h: &mut Harness) -> usize {
    h.rows()
        .iter()
        .filter(|r| r.state == State::Completed)
        .count()
}

fn failed(h: &mut Harness) -> bool {
    h.rows().iter().any(|r| matches!(r.state, State::Failed(_)))
}

fn running(h: &mut Harness) -> bool {
    h.rows().iter().any(|r| r.state == State::Running)
}

/// What the system was asked to open, show or put in the Trash, so far.
fn opened(h: &Harness) -> Vec<String> {
    h.fake.opened.lock().unwrap().clone()
}

/// Every file under `dir`.
fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// A sign-in's password made for the test, not written in it.
fn password() -> String {
    format!("pw-{}-{}", std::process::id(), line!())
}

#[test]
fn add_url_downloads_the_file_announces_it_once_and_says_it_is_complete() {
    let world = world();
    let mut h = world.harness_with(world.settings_with(|s| s.general.show_complete = true));
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", |h| completed(h) == 1);
    let row = h.rows()[0].clone();
    assert_eq!(row.filename, "file.zip");
    assert_eq!(
        std::fs::read(&row.path).unwrap(),
        payload(),
        "the saved bytes are not the served ones"
    );
    h.until("Download complete", |h| h.dialog_open("Download complete"));
    h.until("the announcement", |h| {
        !h.fake.notified.lock().unwrap().is_empty()
    });
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    assert_eq!(
        *h.fake.notified.lock().unwrap(),
        ["file.zip"],
        "announced not exactly once"
    );
    h.click_as_person("Open", 0);
    h.until("the file to be opened", |h| !opened(h).is_empty());
    assert_eq!(opened(&h), std::slice::from_ref(&row.path));
    h.until("Download complete to close", |h| {
        !h.dialog_open("Download complete")
    });
}

#[test]
fn a_newer_release_offers_its_installer_as_a_download_in_dm() {
    use dm_shell::app::update::{Installer, Update};
    let world = world();
    // The local server's files stand in for the release's installer.
    let sample = world.dir.path().join("sample");
    std::fs::write(&sample, payload()).unwrap();
    let sha256 = world
        .runtime
        .block_on(dm_core::digest(&sample, dm_core::Algorithm::Sha256))
        .unwrap();
    let offer = |h: &mut Harness, url: String, sha256: String| {
        h.view(|view| {
            *view.threads.checked.lock().unwrap() =
                Update::Newer("9.9.9".into(), Some(Installer { url, sha256 }))
        });
        h.run(Command::About);
        h.until_shown("Download the update");
        h.click("Download the update");
    };
    let mut h = world.harness();
    let installer = world.url("file.zip");
    offer(&mut h, installer.clone(), sha256);
    h.until("the installer to be fetched", |h| {
        h.rows()
            .iter()
            .any(|r| r.url == installer && r.state == State::Completed && r.verified)
    });
    // Whole and checked: offered for installing, dm closing for it.
    h.until_shown("The dm update is downloaded and checked.");
    let path = h.rows()[0].path.clone();
    if cfg!(target_os = "linux") {
        h.click_as_person("Open folder", 0);
        h.until("the folder to be shown", |h| {
            opened(h).contains(&format!("reveal {path}"))
        });
    } else {
        h.click_as_person("Install now", 0);
        h.until("the installer to be opened", |h| opened(h).contains(&path));
    }
    assert!(!h.shows("The dm update is downloaded and checked."));

    // One that does not match what the release published is not kept.
    let mut h = world.harness();
    let other = world.url("other.pdf");
    offer(&mut h, other.clone(), "0".repeat(64));
    h.until("the wrong installer to fail", |h| {
        h.rows()
            .iter()
            .any(|r| r.url == other && matches!(r.state, State::Failed(_)))
    });
    let row = h.rows().into_iter().find(|r| r.url == other).unwrap();
    assert!(row.path.is_empty() || !std::path::Path::new(&row.path).exists());
    let left: Vec<_> = walk(&world.settings().folders.download_dir)
        .into_iter()
        .filter(|p| p.ends_with("other.pdf"))
        .collect();
    assert!(
        left.is_empty(),
        "the mismatched installer was kept: {left:?}"
    );
    assert!(!h.shows("The dm update is downloaded and checked."));
}

/// A download caught from the browser opens Download details where Options says so, and
/// starts at once where it does not, whatever the panel's own setting.
#[test]
fn a_caught_download_asks_where_to_save_by_its_own_setting() {
    let world = world();
    let mut h = world.harness_with(world.settings_with(|s| {
        s.general.ask_panel_downloads = false;
        s.general.ask_caught_downloads = true;
    }));
    let caught = |h: &mut Harness, name: &str| {
        h.hand_over(dm_shell::host::Request {
            url: world.url(name),
            referrer: "https://e.com/page".into(),
            ask: true,
            caught: true,
            ..Default::default()
        });
    };
    caught(&mut h, "file.zip");
    h.until("Download details", |h| h.dialog_open("Download details"));
    assert!(h.rows().is_empty(), "started before asking");
    h.click("Start now");
    h.until("the row", |h| h.rows().len() == 1);

    h.apply(|s| {
        s.general.ask_panel_downloads = true;
        s.general.ask_caught_downloads = false;
    });
    caught(&mut h, "other.pdf");
    h.until("it to start at once", |h| h.rows().len() == 2);
    assert!(!h.dialog_open("Download details"));
}

/// View › Drop target puts the floating drop target up, over every other window, and takes it
/// away; the choice is kept in the settings.
#[test]
fn view_drop_target_shows_and_hides_the_floating_target() {
    let world = world();
    let mut h = world.harness();
    let open = |h: &mut Harness| {
        h.view(|view| view.windows.drop_window)
            .is_some_and(|w| h.cx.update(|cx| cx.windows().contains(&w)))
    };
    h.click("View");
    h.click("Drop target");
    h.until("the drop target", |h| open(h));
    assert!(
        h.view(|view| view.backend.settings()).drop_target,
        "not kept"
    );
    h.click("View");
    h.click("Drop target");
    h.until("the drop target to go", |h| !open(h));
    assert!(!h.view(|view| view.backend.settings()).drop_target);
}

#[test]
fn download_later_waits_in_the_main_queue_until_start_queue() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| h.rows().len() == 1);
    let row = h.rows()[0].clone();
    assert_eq!(row.state, State::Paused);
    assert_eq!(row.queue, dm_core::MAIN_QUEUE);
    // The side panel's place (the row's Queue cell says the name too, gaps row E7).
    h.update(|view, cx| {
        view.choose(dm_shell::app::places::Place::Queue(
            dm_core::MAIN_QUEUE.into(),
        ));
        cx.notify();
    });
    h.until("the Main queue to be shown", |h| {
        h.view(|view| view.list.place.clone())
            == dm_shell::app::places::Place::Queue(dm_core::MAIN_QUEUE.into())
    });
    h.click("Start queue");
    h.until("the queue's download to complete", |h| completed(h) == 1);
}

/// "Download all links" offers a link already in the list unticked, and says so: it was
/// fetched again as `name (1).zip` without a word.
#[test]
fn a_pages_link_already_listed_is_offered_unticked_and_said() {
    let world = world();
    let mut h = world.harness_with(world.settings_with(|s| s.general.ask_panel_downloads = true));
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| h.rows().len() == 1);
    h.hand_over(dm_shell::host::Request {
        url: world.url("page.html"),
        referrer: world.url("page.html"),
        links: vec![world.url("file.zip"), world.url("other.pdf")],
        ask: true,
        ..Default::default()
    });
    h.until("Download links", |h| h.dialog_open("Download links"));
    h.until("the listed one to be said", |h| {
        h.texts()
            .iter()
            .any(|t| t.contains("(already in the list)"))
    });
    // Only the other is ticked: Start now adds it alone.
    h.click("Start now");
    h.until("the ticked link to be added", |h| h.rows().len() == 2);
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    let urls: Vec<String> = h.rows().iter().map(|r| r.url.clone()).collect();
    assert_eq!(urls.len(), 2, "{urls:?}");
    assert!(urls.contains(&world.url("other.pdf")), "{urls:?}");
}

#[test]
fn delete_waits_for_a_download_to_stop() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("the row to start", running);
    h.select("stalled.bin");
    assert!(
        !h.enabled(Command::Remove),
        "Delete is offered for a download still running"
    );
    h.run(Command::Remove);
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    assert_eq!(h.rows().len(), 1, "a running download was deleted");
}

/// Copy link puts the row's address on the clipboard, without the sign-in that is the row's
/// own.
#[test]
fn copy_link_copies_the_address_without_its_sign_in() {
    let world = world();
    let mut h = world.harness();
    let password = password();
    let plain = world.url("private.bin");
    let signed = plain.replace("http://", &format!("http://me:{password}@"));
    h.view(|view| {
        view.backend
            .add_with(
                &signed,
                dm_core::AddOptions {
                    later: true,
                    ..Default::default()
                },
            )
            .unwrap()
    });
    h.until("the row", |h| h.rows().len() == 1);
    h.select("private.bin");
    assert!(h.enabled(Command::CopyLink));
    h.run(Command::CopyLink);
    let copied = h.fake.clipboard.lock().unwrap().clone();
    let copied = copied.unwrap_or_default();
    assert_eq!(copied, plain);
    assert!(!copied.contains(&password), "the sign-in went with it");
}

#[test]
fn a_batch_adds_every_address_the_star_makes() {
    let world = world();
    let mut h = world.harness();
    h.run(Command::AddSeries);
    h.until("Add numbered series", |h| {
        h.dialog_open("Add numbered series")
    });
    h.type_text(&world.url("part*.bin"));
    h.click_beside("From");
    h.type_text("1");
    h.click_beside("To");
    h.type_text("12");
    h.until("the count", |h| {
        h.texts().iter().any(|t| t.contains("12 addresses"))
    });
    // Each address is listed, not only the first.
    for n in ["part1.bin", "part2.bin", "part5.bin"] {
        assert!(h.shows(&world.url(n)), "{n} is not in the preview");
    }
    h.click("Add for later");
    h.until("the rows", |h| h.rows().len() == 12);
    let mut urls: Vec<String> = h.rows().iter().map(|r| r.url.clone()).collect();
    urls.sort_by_key(|u| u.len());
    assert!(urls[0].ends_with("/part1.bin") && urls[11].ends_with("/part12.bin"));
}

/// Add URL remembers the addresses added through it, never with a login, offers them again,
/// and forgets them on Clear history.
#[test]
fn add_url_remembers_addresses_without_their_login_and_clears_them() {
    let world = world();
    let mut h = world.harness();
    let url = world.url("file.zip");
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(&url);
    h.click("Needs a sign-in");
    h.until_shown("User name");
    h.click("User name");
    h.type_text("me");
    h.click("Next");
    h.until("Download details", |h| h.dialog_open("Download details"));
    assert_eq!(h.view(|view| view.backend.settings()).recent_urls, [url]);
    h.click("Cancel");
    h.until("Download details to close", |h| {
        !h.dialog_open("Download details")
    });
    h.click("Add URL");
    h.until_shown("Recent");
    h.click("Clear history");
    h.until("the history to go", |h| {
        h.view(|view| view.backend.settings())
            .recent_urls
            .is_empty()
    });
    // Not for good: it says so, and Undo remembers them again.
    h.until_shown("History cleared.");
    h.click("Undo");
    h.until("the history back", |h| {
        h.view(|view| view.backend.settings()).recent_urls.len() == 1
    });
}

/// Options › General saves the settings to a file without any password, and loads them back
/// into the dialog, to be used on OK.
#[test]
fn settings_are_saved_to_a_file_without_passwords_and_loaded_back() {
    let world = world();
    let mut settings = world.settings();
    settings.downloads.connections = 5;
    settings.advanced.proxy.username = "someone".into();
    settings.advanced.proxy.password = "not-in-the-file".into();
    let mut h = world.harness_with(settings);
    h.run(Command::Options);
    h.until("Options", |h| h.dialog_open("Options"));
    let file = world.dir.path().join("dm-settings.json");
    h.scroll_to("Save settings to a file…");
    crate::platform::services::choose_ahead(file.clone());
    h.click("Save settings to a file…");
    h.until("the file", |_| file.is_file());
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"connections\": 5"), "{text}");
    assert!(
        !text.contains("not-in-the-file"),
        "a password went into the file"
    );
    h.click("Cancel");
    h.until("Options to close", |h| !h.dialog_open("Options"));
    h.apply(|s| s.downloads.connections = 2);
    h.run(Command::Options);
    h.until("Options", |h| h.dialog_open("Options"));
    h.scroll_to("Load settings from a file…");
    crate::platform::services::choose_ahead(file.clone());
    h.click("Load settings from a file…");
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    h.click("OK");
    h.until("Options to close", |h| !h.dialog_open("Options"));
    let now = h.view(|view| view.backend.settings());
    assert_eq!(now.downloads.connections, 5);
    assert_eq!(
        now.advanced.proxy.password, "not-in-the-file",
        "this computer's password was lost"
    );
}

/// A drop with no link in it says how links are added, rather than nothing; the empty list
/// says so too.
#[test]
fn a_drop_with_no_link_says_how_to_add_one() {
    let world = world();
    let mut h = world.harness();
    let hint = "Drop a link, .torrent, .url, .webloc or text file of links here.";
    assert!(h.shows(hint), "{:?}", h.texts());
    let file = world.dir.path().join("picture.png");
    std::fs::write(&file, [0u8, 1, 2, 3]).unwrap();
    // Dropped on the list, as the system hands a drop to the window.
    let at = gpui::point(gpui::px(400.0), gpui::px(300.0));
    h.cx.update(|cx| {
        let _ = h.window.update(cx, |_, window, cx| {
            let paths = gpui::ExternalPaths(vec![file].into());
            window.dispatch_event(
                gpui::PlatformInput::FileDrop(gpui::FileDropEvent::Entered {
                    position: at,
                    paths,
                }),
                cx,
            );
            window.dispatch_event(
                gpui::PlatformInput::FileDrop(gpui::FileDropEvent::Submit { position: at }),
                cx,
            );
        });
    });
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert!(h.rows().is_empty());
    assert!(
        h.said().contains("Drop a link"),
        "the drop was ignored without a word: {:?}",
        h.said()
    );
}

/// A .torrent on this computer is added by its path or its file address in Add URL; only a
/// drop took it.
#[test]
fn add_url_takes_a_torrent_on_this_computer() {
    let world = world();
    let mut h = world.harness();
    let file = world.dir.path().join("a set.torrent");
    std::fs::write(&file, b"d4:infodee").unwrap();
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(file.to_str().unwrap());
    h.click("Next");
    h.until("Download details", |h| h.dialog_open("Download details"));
    let address = dm_core::file_url(&file).unwrap();
    assert!(h.shows(&address), "{:?}", h.texts());
}

/// A stream server's address goes in through Add URL like any other, is named as it will be
/// saved, and is queued; dm's own stream client records it (the engine's `tests/live.rs`).
#[test]
fn a_stream_servers_address_is_taken_through_add_url() {
    let world = world();
    let mut h = world.harness();
    h.add_url("rtmp://127.0.0.1:1/live/show", "Add for later");
    h.until("the row", |h| h.rows().len() == 1);
    let rows = h.rows();
    assert_eq!(rows[0].url, "rtmp://127.0.0.1:1/live/show");
    h.until("its name", |h| h.texts().iter().any(|t| t.contains("show")));
    assert!(dm_shell::app::links::looks_downloadable("rtmp://h/live/x").is_some());
    assert!(dm_shell::app::links::looks_downloadable("mms://h/clip.asf").is_some());
}

/// Escape closes Download complete as its Close does, and a ticked "Don't show this window
/// again" is kept either way.
#[test]
fn escape_keeps_a_ticked_dont_show_again_as_close_does() {
    let world = world();
    let mut h = world.harness_with(world.settings_with(|s| s.general.show_complete = true));
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", |h| completed(h) == 1);
    h.until("Download complete", |h| h.dialog_open("Download complete"));
    h.click("Don't show this window again");
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert!(h.view(|view| view.backend.settings()).general.show_complete);
    h.key("escape", &[]);
    h.until("the window to close", |h| {
        !h.dialog_open("Download complete")
    });
    assert!(!h.view(|view| view.backend.settings()).general.show_complete);
}

/// A double-click opens a finished file, as a file manager's does.
#[test]
fn a_new_install_opens_a_finished_file_on_a_double_click() {
    assert_eq!(Settings::default().general.double_click, "open");
}

/// Several links pasted into Add URL, a line each (the field makes them one line with a space
/// between), are one batch in Download details, not a warning to add them one at a time.
#[test]
fn five_links_pasted_into_add_url_are_one_batch() {
    let world = world();
    let mut h = world.harness();
    let links: Vec<String> = (1..=5)
        .map(|i| world.url(&format!("batch-{i}.bin")))
        .collect();
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(&links.join(" "));
    h.click("Next");
    h.until("Download details", |h| h.dialog_open("Download details"));
    assert!(
        !h.texts().iter().any(|t| t.contains("looks like two links")),
        "{:?}",
        h.texts()
    );
    h.click("Add for later");
    h.until("the batch", |h| h.rows().len() == 5);
    let mut asked: Vec<String> = h.rows().iter().map(|r| r.url.clone()).collect();
    asked.sort();
    assert_eq!(asked, links, "the five links were not one batch");
}

/// A very big download says once that the Scheduler can run it at night, and never again.
#[test]
fn a_very_big_download_is_told_about_the_scheduler_once() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("it to run", running);
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert!(
        !h.view(|view| view.backend.settings()).hinted_big,
        "said for a small file"
    );
    // The hint reads the running rows the reads keep (`Known::busy`, gaps row 181), not the
    // list's: the size is told to those.
    h.view(|view| {
        assert!(!view.known.busy.is_empty(), "no running row kept");
        for row in view.known.busy.values_mut() {
            std::sync::Arc::make_mut(row).progress.total = Some(6 << 30);
        }
        view.big_download_hint();
    });
    assert!(h.view(|view| view.backend.settings()).hinted_big);
    assert!(
        h.said().contains("Scheduler can run it at night"),
        "{}",
        h.said()
    );
    // Said, so not said again: the words are cleared and it stays quiet.
    h.view(|view| {
        view.status.said = None;
        view.big_download_hint();
    });
    assert!(h.said().is_empty(), "said a second time");
}

/// Describing the window to a script (`ui.inspect`) with a long list took time that grew with
/// the square of the list (7 s at 5 000 rows): each row's name looked through every other row
/// for its namesakes. Added in one call, 3 000 links are also checked against the list once,
/// not each against every row.
#[test]
fn describing_a_long_list_takes_a_moment_not_seconds() {
    let world = world();
    let mut h = world.harness();
    let urls: Vec<String> = (0..3000)
        .map(|n| world.url(&format!("many/f{n}.bin")))
        .collect();
    let started = std::time::Instant::now();
    let added = h
        .call(
            "downloads.add",
            serde_json::json!({ "urls": urls, "later": true }),
        )
        .unwrap();
    assert_eq!(added["added"], 3000);
    let adding = started.elapsed();
    h.until("the rows", |h| h.rows().len() == 3000);
    let started = std::time::Instant::now();
    let seen = h.call("ui.inspect", serde_json::json!({})).unwrap();
    let inspecting = started.elapsed();
    assert_eq!(seen["rows"].as_array().unwrap().len(), 3000);
    assert!(
        adding < Duration::from_secs(10) && inspecting < Duration::from_secs(2),
        "3000 links took {adding:?} to add and {inspecting:?} to describe"
    );
}

/// Under the Remove question the list takes no clicks: a row selected there was not the one
/// removed. The question names its row.
#[test]
fn the_list_behind_the_remove_question_takes_no_clicks() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Add for later");
    h.add_url(&world.url("other.pdf"), "Add for later");
    h.until("both rows", |h| h.rows().len() == 2);
    h.select("file.zip");
    let chosen = h.view(|view| view.list.selected.clone());
    h.run(Command::Remove);
    h.until("the question", |h| h.dialogs().len() == 1);
    assert!(
        h.texts().iter().filter(|t| *t == "file.zip").count() >= 2,
        "the question names no row: {:?}",
        h.texts()
    );
    // On macOS and Windows the question is the system's sheet or owned dialog, which holds
    // the window's clicks itself, and the test platform has none; elsewhere dm holds them
    // (`Downloads::holder`).
    if cfg!(not(any(target_os = "macos", windows))) {
        h.click("other.pdf");
        assert_eq!(
            h.view(|view| view.list.selected.clone()),
            chosen,
            "a row was selected under the question"
        );
    }
}

/// The link Add URL opens with is selected, so a link pasted over it replaces it; two
/// addresses run together are refused with a reason, and the address keeps the keyboard.
#[test]
fn add_url_selects_its_link_and_refuses_two_run_together() {
    let world = world();
    let mut h = world.harness();
    let first = world.url("file.zip");
    let second = world.url("other.pdf");
    *h.fake.clipboard.lock().unwrap() = Some(first.clone());
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(&format!("{first}{second}"));
    h.click("Next");
    h.until("the refusal", |h| {
        h.texts().iter().any(|t| t.contains("looks like two links"))
    });
    assert!(!h.dialog_open("Download details"));
    assert!(h.rows().is_empty());
    // The address keeps the keyboard, chosen: what is typed next replaces it.
    h.type_text(&second);
    h.click("Next");
    h.until("Download details", |h| h.dialog_open("Download details"));
    assert!(h.shows(&second), "{:?}", h.texts());
    assert!(!h.shows(&format!("{first}{second}")));
}

/// The limiter's field takes what is typed in place of what it held; the toolbar's buttons
/// stay put when Resume becomes Retry; the progress window says what a refusal leaves
/// unknown.
#[test]
fn the_small_things_of_round_eight() {
    let world = world();
    // The limiter: 500 typed over 64 is 500, not 64500.
    let mut h = world.harness_with(world.settings_with(|s| {
        s.advanced.speed_limit = 64;
        s.general.show_progress = true;
    }));
    h.run(Command::SpeedLimiterSettings);
    h.until("the limiter", |h| h.dialogs().len() == 1);
    h.type_text("500");
    h.key("enter", &[]);
    h.until("the limit", |h| {
        h.view(|view| view.backend.settings()).advanced.speed_limit == 500
    });
    h.apply(|s| s.advanced.speed_limit = 0);

    // The toolbar: one refused, one finished.
    h.add_url(&world.url("secret.zip"), "Start now");
    h.until("the refusal", failed);
    // The server answered, with a refusal: said as one.
    h.until_shown("No — the server refused it");
    h.apply(|s| s.general.show_progress = false);
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("it to finish", |h| completed(h) == 1);
    let delete_at = |h: &mut Harness| {
        let texts = h.call("ui.texts", serde_json::Value::Null).unwrap();
        texts["texts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["text"] == "Delete" && t["window"] == "main")
            .map(|t| t["rect"][0].as_f64().unwrap())
            .expect("Delete is on the toolbar")
    };
    h.select("file.zip");
    h.until_shown("Resume");
    let resume = delete_at(&mut h);
    h.select("secret.zip");
    h.until_shown("Retry");
    let retry = delete_at(&mut h);
    assert_eq!(resume, retry, "Delete moved when Resume became Retry");
}
