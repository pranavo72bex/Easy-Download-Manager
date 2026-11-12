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
    let mut h = world.harness_with(Settings {
        show_complete: true,
        ..world.settings()
    });
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
            *view.checked.lock().unwrap() =
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
    let left: Vec<_> = walk(&world.settings().download_dir)
        .into_iter()
        .filter(|p| p.ends_with("other.pdf"))
        .collect();
    assert!(
        left.is_empty(),
        "the mismatched installer was kept: {left:?}"
    );
    assert!(!h.shows("The dm update is downloaded and checked."));
}

/// Extract here and Extract to…; cancelling the folder panel is not tried here, as the test
/// platform's panel is the system's own.
#[test]
fn a_finished_zip_is_extracted_beside_itself_and_a_hostile_one_is_refused() {
    use std::io::Write;
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("file.zip to finish", |h| completed(h) == 1);
    h.select("file.zip");
    let path = std::path::PathBuf::from(h.row("file.zip").path);
    assert!(h.enabled(Command::ExtractHere));

    // The served bytes are not a ZIP: said so, and nothing is made.
    h.run(Command::ExtractHere);
    h.until("the refusal", |h| h.said().starts_with("Not extracted"));

    let write = |names: &[&str]| {
        let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for name in names {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(b"inside").unwrap();
        }
        w.finish().unwrap();
    };
    write(&["a.txt", "../escape.txt"]);
    h.run(Command::ExtractHere);
    h.until("the hostile archive's refusal", |h| {
        h.said().contains("leave its folder")
    });
    let folder = path.with_extension("");
    assert!(!folder.exists(), "a refused archive leaves no folder");
    assert!(!path.parent().unwrap().join("escape.txt").exists());

    write(&["a.txt", "sub/b.txt"]);
    h.run(Command::ExtractHere);
    h.until("the extraction", |h| {
        h.said().starts_with("Extracted 2 files")
    });
    assert_eq!(std::fs::read(folder.join("sub/b.txt")).unwrap(), b"inside");
    assert!(path.exists(), "the archive is kept");

    // Extract to…: into the folder the person picks, a new folder inside it.
    let elsewhere = tempfile::tempdir().unwrap();
    assert!(h.enabled(Command::ExtractTo));
    h.view(|view| view.say(String::new()));
    crate::services::choose_ahead(elsewhere.path().to_path_buf());
    h.run(Command::ExtractTo);
    let name = path.file_stem().unwrap().to_owned();
    h.until("the extraction elsewhere", |_| {
        elsewhere.path().join(&name).join("a.txt").is_file()
    });
    h.until("it to be said", |h| {
        h.said().starts_with("Extracted 2 files")
    });
    assert_eq!(
        std::fs::read(elsewhere.path().join(&name).join("a.txt")).unwrap(),
        b"inside"
    );
}

#[test]
fn a_video_half_fetched_is_previewed_from_what_is_on_disk() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("half.mp4"), "Start now");
    h.until("the first bytes to arrive", |h| {
        h.rows().iter().any(|r| r.progress.downloaded >= 128 * 1024)
    });
    h.select("half.mp4");
    assert!(!h.enabled(Command::OpenFile), "not finished");
    assert!(h.enabled(Command::Preview));
    h.run(Command::Preview);
    let preview = |h: &Harness| opened(h).into_iter().find(|p| p.ends_with("preview.mp4"));
    h.until("the preview to be opened", |h| preview(h).is_some());
    let preview = preview(&h).unwrap();
    let bytes = std::fs::read(&preview).unwrap();
    // From the start, all of it but what is still in the engine's write buffer, 64 KiB at
    // most (the staged file is as long as the whole, zeros past that).
    assert!(bytes.len() >= 32 * 1024, "{} bytes", bytes.len());
    assert!(bytes[..32 * 1024].iter().all(|b| *b == 9));

    // A file no player shows is not offered.
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("its row", |h| h.rows().len() == 2);
    h.select("stalled.bin");
    assert!(!h.enabled(Command::Preview));

    // Removing the row takes the preview with its parts.
    let id = h.row("half.mp4").id;
    h.view(|view| view.backend.remove(id));
    h.until("the preview to go with its row", |_| {
        !std::path::Path::new(&preview).exists()
    });
}

#[test]
fn a_finished_file_deleted_since_says_so_and_is_not_offered_to_open() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", |h| completed(h) == 1);
    let row = h.rows()[0].clone();
    h.select("file.zip");
    assert!(h.enabled(Command::OpenFile));

    std::fs::remove_file(&row.path).unwrap();
    h.until_shown("File missing");
    assert!(!h.enabled(Command::OpenFile));
    assert!(!h.enabled(Command::OpenWith));
    assert!(
        h.enabled(Command::OpenFolder),
        "Open folder still answers: it says the file is gone"
    );
    assert!(h.enabled(Command::Redownload));
}

/// A download caught from the browser opens Download details where Options says so, and
/// starts at once where it does not, whatever the panel's own setting.
#[test]
fn a_caught_download_asks_where_to_save_by_its_own_setting() {
    let world = world();
    let mut h = world.harness_with(Settings {
        ask_panel_downloads: false,
        ask_caught_downloads: true,
        ..world.settings()
    });
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
        s.ask_panel_downloads = true;
        s.ask_caught_downloads = false;
    });
    caught(&mut h, "other.pdf");
    h.until("it to start at once", |h| h.rows().len() == 2);
    assert!(!h.dialog_open("Download details"));
}

/// A download's progress window limits that download alone: ticked, it takes the KB/s shown;
/// unticked, the limit goes.
#[test]
fn the_progress_window_limits_its_own_download() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("the row", |h| h.rows().len() == 1);
    let id = h.rows()[0].id;
    h.update(|view, cx| view.show_progress(id, cx));
    h.until_shown("Limit this download to");
    h.click("Limit this download to");
    h.until("the limit", |h| h.rows()[0].speed_limit == 500);
    h.click("Limit this download to");
    h.until("the limit to go", |h| h.rows()[0].speed_limit == 0);
}

/// View › Drop target puts the floating drop target up, over every other window, and takes it
/// away; the choice is kept in the settings.
#[test]
fn view_drop_target_shows_and_hides_the_floating_target() {
    let world = world();
    let mut h = world.harness();
    let open = |h: &mut Harness| {
        h.view(|view| view.drop_window)
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

/// Open with… asks for an application and opens the file with it. A cancelled panel, and a
/// desktop with none, are the system's own panel's to show: not tried here.
#[test]
fn open_with_asks_for_an_application_and_opens_the_file_with_it() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", |h| completed(h) == 1);
    let row = h.rows()[0].clone();
    h.select("file.zip");
    assert!(h.enabled(Command::OpenWith));
    let app = std::path::PathBuf::from("/Applications/Archive Utility.app");
    crate::services::choose_ahead(app.clone());
    h.run(Command::OpenWith);
    let wanted = format!("{} with {}", row.path, app.display());
    h.until("the file to be opened with it", |h| {
        opened(h).contains(&wanted)
    });
    assert_eq!(opened(&h), [wanted]);
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
    h.click("Main queue");
    h.until("the Main queue to be shown", |h| {
        h.view(|view| view.place.clone())
            == dm_shell::app::places::Place::Queue(dm_core::MAIN_QUEUE.into())
    });
    h.click("Start Queue");
    h.until("the queue's download to complete", |h| completed(h) == 1);
}

/// Move/Rename: a name taken in that folder is said, with the name given instead; Cancel
/// closes it untouched. (pack.zip is renamed onto file.zip: typing replaces the name and
/// keeps its ending, as Finder's rename does.)
#[test]
fn renaming_onto_a_taken_name_says_what_it_is_called() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("pack.zip"), "Start now");
    h.until("both to complete", |h| completed(h) == 2);
    let pack = h.row("pack.zip").id;
    h.select("pack.zip");
    h.run(Command::MoveRename);
    h.until("Move/Rename", |h| h.dialog_open("Move/Rename"));
    h.click("Cancel");
    h.until("Cancel to close it", |h| !h.dialog_open("Move/Rename"));
    assert_eq!(h.row("pack.zip").id, pack);
    h.run(Command::MoveRename);
    h.until("Move/Rename", |h| h.dialog_open("Move/Rename"));
    h.type_text("file");
    h.click("OK");
    h.until("it to be said", |h| {
        h.said().contains("is taken in that folder")
    });
    let given = h
        .rows()
        .into_iter()
        .find(|r| r.id == pack)
        .unwrap()
        .filename;
    assert_ne!(given, "file.zip", "the other file's name was taken over");
    let said = format!("file.zip is taken in that folder, so it is called {given}.");
    assert_eq!(h.said(), said);
    // In the status bar, where a screen reader reads it as it appears.
    h.until("the status bar to say it", |h| {
        h.texts().iter().any(|t| t.contains(&said))
    });
}

/// "Download all links" offers a link already in the list unticked, and says so: it was
/// fetched again as `name (1).zip` without a word.
#[test]
fn a_pages_link_already_listed_is_offered_unticked_and_said() {
    let world = world();
    let mut h = world.harness_with(Settings {
        ask_panel_downloads: true,
        ..world.settings()
    });
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
    let copied =
        h.cx.read_from_clipboard()
            .and_then(|c| c.text())
            .unwrap_or_default();
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

#[test]
fn a_failed_download_plays_the_failure_sound_when_asked() {
    let world = world();
    let mut h = world.harness_with(Settings {
        sound_on_failure: true,
        ..world.settings()
    });
    // Nothing is served at this name: a 404, failed and not retried.
    h.add_url(&world.url("missing.bin"), "Start now");
    h.until("the row failed", failed);
    h.until("the sound", |h| opened(h).iter().any(|o| o == "alert"));
    assert!(!opened(&h).iter().any(|o| o == "chime"), "{:?}", opened(&h));
    // Once: a redraw does not sound it again.
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    assert_eq!(opened(&h).iter().filter(|o| *o == "alert").count(), 1);
}

/// A double-click on a link in the browser hands it over twice in a moment: one row, not
/// `name (1)` and `name (2)`.
#[test]
fn the_same_address_handed_twice_at_once_is_taken_once() {
    let world = world();
    let mut h = world.harness();
    let url = world.url("file.zip");
    h.view(|view| {
        let mut handed = view.backend.handed.lock().unwrap();
        for _ in 0..2 {
            handed.push_back(dm_shell::host::Request {
                url: url.clone(),
                ..Default::default()
            });
        }
    });
    h.until("the row", |h| h.rows().len() == 1);
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    assert_eq!(h.rows().len(), 1);
}

/// Remove can take a finished file off the disk with its row, when asked; left alone, the
/// file stays.
#[test]
fn remove_deletes_the_file_too_only_when_asked() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("other.pdf"), "Start now");
    h.until("both to complete", |h| completed(h) == 2);
    let (zip, pdf) = (h.row("file.zip").path, h.row("other.pdf").path);
    h.select("file.zip");
    h.run(Command::Remove);
    h.until("the row to go", |h| h.rows().len() == 1);
    assert!(
        std::path::Path::new(&zip).is_file(),
        "the file went without being asked"
    );
    // Delete file is the one that asks, and says what happens to the file.
    h.select("other.pdf");
    h.run(Command::DeleteFile);
    h.until_shown("Yes, delete");
    h.click("Yes, delete");
    h.until("the row and its file to go", |h| h.rows().is_empty());
    h.until("the file to go", |_| !std::path::Path::new(&pdf).exists());
    assert!(
        opened(&h).contains(&format!("trash {pdf}")),
        "the file was erased, not put in the Recycle Bin or Trash"
    );
}

/// "Delete file after": the row says how long its file has left, and once the time comes the
/// file goes to the Trash and the row with it.
#[test]
fn a_file_given_a_time_goes_to_the_trash_when_it_comes() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("other.pdf"), "Start now");
    h.until("both to complete", |h| completed(h) == 2);
    let (zip, pdf) = (h.row("file.zip"), h.row("other.pdf"));
    // An hour: said in the row, and the file stays.
    h.view(|view| view.backend.set_delete_after(pdf.id, Some(3600)));
    h.until("the row to say so", |h| {
        h.texts().iter().any(|t| t.contains("to the Trash in"))
    });
    assert!(std::path::Path::new(&pdf.path).is_file());
    // A second: gone to the Trash, the row with it; the other row stays.
    h.view(|view| view.backend.set_delete_after(pdf.id, Some(1)));
    h.until("the file to be deleted", |h| {
        h.rows().iter().all(|r| r.id != pdf.id)
    });
    assert!(!std::path::Path::new(&pdf.path).exists(), "the file stayed");
    assert!(
        opened(&h).contains(&format!("trash {}", pdf.path)),
        "the file was erased, not put in the Trash"
    );
    assert!(
        std::path::Path::new(&zip.path).is_file(),
        "the other went too"
    );
    assert_eq!(h.rows().len(), 1);
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
    h.until_shown("Login");
    h.click("Login");
    h.type_text("me");
    h.click("OK");
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
}

/// Options › General saves the settings to a file without any password, and loads them back
/// into the dialog, to be used on OK.
#[test]
fn settings_are_saved_to_a_file_without_passwords_and_loaded_back() {
    let world = world();
    let mut settings = world.settings();
    settings.connections = 5;
    settings.proxy.username = "someone".into();
    settings.proxy.password = "not-in-the-file".into();
    let mut h = world.harness_with(settings);
    h.run(Command::Options);
    h.until("Options", |h| h.dialog_open("Options"));
    let file = world.dir.path().join("dm-settings.json");
    h.scroll_to("Save settings to a file…");
    crate::services::choose_ahead(file.clone());
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
    h.apply(|s| s.connections = 2);
    h.run(Command::Options);
    h.until("Options", |h| h.dialog_open("Options"));
    h.scroll_to("Load settings from a file…");
    crate::services::choose_ahead(file.clone());
    h.click("Load settings from a file…");
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    h.click("OK");
    h.until("Options to close", |h| !h.dialog_open("Options"));
    let now = h.view(|view| view.backend.settings());
    assert_eq!(now.connections, 5);
    assert_eq!(
        now.proxy.password, "not-in-the-file",
        "this computer's password was lost"
    );
}

/// The progress window's Cancel download asks first; Keep downloading (and Enter) leaves it
/// running.
#[test]
fn cancel_download_in_the_progress_window_asks_first() {
    let world = world();
    let mut h = world.harness();
    h.view(|view| view.backend.add(&world.url("stalled.bin")).unwrap());
    h.until("it to run", running);
    let id = h.rows()[0].id;
    h.update(|view, cx| view.show_progress(id, cx));
    h.until_shown("Cancel download");
    h.click("Cancel download");
    h.until_shown("Cancel this download?");
    h.key("enter", &[]);
    h.until("the question to go", |h| !h.shows("Cancel this download?"));
    assert_ne!(h.rows()[0].state, State::Cancelled, "Enter cancelled it");
    h.click("Cancel download");
    h.until_shown("Yes, cancel");
    h.click("Yes, cancel");
    h.until("it to be cancelled", |h| {
        h.rows()[0].state == State::Cancelled
    });
}

#[test]
fn a_progress_window_shows_each_connection() {
    let world = world();
    let mut h = world.harness_with(Settings {
        show_progress: true,
        ..world.settings()
    });
    h.add_url(&world.url("stalled.bin"), "Start now");
    // Running comes a moment before the plan is written; until then the window says "Unknown
    // until the server answers", which is right.
    h.until("the download to be planned", |h| {
        h.view(|view| {
            view.rows
                .iter()
                .any(|r| r.state == State::Running && view.backend.resumable(r.id).is_some())
        })
    });
    h.until("its progress window", |h| {
        h.dialogs().iter().any(|d| d.contains("stalled.bin"))
    });
    h.until_shown("Can resume");
    // That server does not serve ranges, and the window says so rather than guessing "Yes"
    // from a known size.
    h.until("the answer", |h| {
        h.texts()
            .iter()
            .any(|t| t.contains("No — a stop starts it again"))
    });
    assert!(h.shows("Connections"), "{:?}", h.texts());
    h.click("Stop download");
    h.until("it to stop", |h| h.rows()[0].state == State::Paused);
}

/// A command run on each finished file, its path added as one argument, whatever characters
/// the name has.
#[cfg(unix)]
#[test]
fn a_finished_download_is_handed_to_the_command_set_to_run_after() {
    use std::os::unix::fs::PermissionsExt;
    let world = world();
    let scratch = tempfile::tempdir().unwrap();
    let record = scratch.path().join("checked");
    let script = scratch.path().join("checker with space");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s|%s' \"$1\" \"$2\" > '{}'\n",
            record.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut h = world.harness_with(Settings {
        on_complete: format!("'{}' --scan", script.display()),
        ..world.settings()
    });
    // Added as a person adds it: a script's click a moment before it finished holds the
    // command back.
    h.view(|view| view.backend.add(&world.url("file.zip")).unwrap());
    h.until("the download to finish", |h| completed(h) == 1);
    let path = h.rows()[0].path.clone();
    h.until_within(
        "the command to be run on the file",
        Duration::from_secs(10),
        &mut |_| std::fs::read_to_string(&record).is_ok_and(|r| !r.is_empty()),
    );
    assert_eq!(
        std::fs::read_to_string(&record).unwrap(),
        format!("--scan|{path}"),
        "the command was not run on the file"
    );
}

/// Several downloads finishing make one "Download complete" window listing each, with its own
/// Open and Open folder.
#[test]
fn downloads_finishing_together_share_one_complete_window() {
    let world = world();
    let mut h = world.harness_with(Settings {
        show_complete: true,
        ..world.settings()
    });
    let urls = vec![
        world.url("file.zip"),
        world.url("other.pdf"),
        world.url("deep.pdf"),
    ];
    h.update(|view, cx| view.add(urls, Default::default(), cx))
        .unwrap();
    h.until("all three to finish", |h| completed(h) == 3);
    h.until_shown("3 downloads finished.");
    let completes = h
        .dialogs()
        .iter()
        .filter(|d| *d == "Download complete")
        .count();
    assert_eq!(completes, 1);
    // Each with its own Open: the one pressed opens its file and leaves the window, the rest
    // stay.
    h.click_as_person("Open", 0);
    h.until("a file to be opened", |h| !opened(h).is_empty());
    let first = opened(&h)[0].clone();
    assert!(
        h.rows().iter().any(|r| r.path == first),
        "{first} is no row's file"
    );
    h.until("the rest to stay", |h| h.shows("2 downloads finished."));
    h.click("Open folder");
    h.until("a folder to be shown", |h| opened(h).len() == 2);
    assert!(opened(&h)[1].starts_with("reveal "));
    h.click("Close");
    h.until("the window to close", |h| {
        !h.dialog_open("Download complete")
    });
}

/// A server that wants a sign-in says where to give one, and Properties opens on its Login.
#[test]
fn a_refused_sign_in_says_where_to_give_one() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("secret.zip"), "Start now");
    h.until("the refusal", failed);
    let row = h.rows()[0].clone();
    let said = dm_desktop_base::rows::status_of(&row);
    // In plain words, saying where a sign-in goes, without the address or the server's page.
    assert!(said.contains("Properties › Login"), "{said}");
    assert!(!said.contains("http"), "{said}");
    h.select("secret.zip");
    // Retry, not Resume, on the toolbar.
    h.until_shown("Retry");
    h.run(Command::Properties);
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.type_text("user");
    h.click("OK");
    h.until("the sign-in to be kept with the row", |h| {
        h.rows()[0].url.contains("user@")
    });
}

/// Save as takes the name the server gives, and leaves one a person typed alone.
#[test]
fn save_as_takes_the_servers_name() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("named"), "Start now");
    // dm asks whether to keep the server's name (`asks.rs`); kept, it is the file's.
    h.until_shown("Name sent by the server");
    h.click("Use report-2026.pdf");
    h.until("it to finish", |h| completed(h) == 1);
    assert!(h.rows()[0].path.ends_with("report-2026.pdf"));
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

/// A link that runs out part-way asks for a fresh one at once, and the fresh one goes on from
/// the parts already fetched.
#[test]
fn a_link_that_runs_out_part_way_asks_for_a_fresh_one() {
    let world = world();
    let mut h = world.harness_with(Settings {
        attempts: 3,
        retry_wait: 0,
        record_minutes: 0,
        ..world.settings()
    });
    h.view(|view| view.backend.add(&world.url("expires.bin")).unwrap());
    h.until("the expired link to fail", failed);
    let row = h.rows()[0].clone();
    assert!(dm_shell::app::rules::link_expired(&row), "{:?}", row.state);
    h.until("a fresh link to be asked for", |h| {
        h.dialog_open("Update download link")
    });
    // The fresh link to the same file: it goes on, and finishes whole.
    h.type_text(&world.url("renewed.bin"));
    h.click("OK");
    h.until("the dialog to close", |h| {
        !h.dialog_open("Update download link")
    });
    h.until("the download to finish", |h| completed(h) == 1);
    let row = h.rows()[0].clone();
    let bytes = std::fs::read(&row.path).unwrap();
    assert_eq!(bytes.len(), 1_000_000);
    assert!(bytes.iter().all(|b| *b == 7));
}

/// A download whose folder cannot be reached (as when its drive is taken out) is paused with
/// why, not failed; the bar over the list offers another folder, and there it finishes, its
/// parts carried over.
#[test]
fn an_unreachable_folder_pauses_and_another_folder_finishes_it() {
    let world = world();
    let blocker = world.dir.path().join("not-a-drive");
    std::fs::write(&blocker, b"a file where the folder should be").unwrap();
    let download_dir = blocker.join("out");
    let mut h = world.harness_with(Settings {
        download_dir: download_dir.clone(),
        ..world.settings()
    });
    h.view(|view| view.backend.add(&world.url("file.zip")).unwrap());
    h.until("the download to pause by itself", |h| {
        h.rows()
            .iter()
            .any(|r| r.state == State::Paused && !r.stalled.is_empty())
    });
    let row = h.rows()[0].clone();
    let status = dm_desktop_base::rows::status_of(&row);
    assert!(status.starts_with("Paused: "), "{status}");
    // The drive back, and a folder on it chosen: inside the download folder, as one chosen
    // while a script drives the window must be (`control_api::folder_refused`), which these
    // tests' clicks are.
    h.until_shown("Choose another folder…");
    std::fs::remove_file(&blocker).unwrap();
    let elsewhere = download_dir.join("elsewhere");
    crate::services::choose_ahead(elsewhere.clone());
    h.click("Choose another folder…");
    h.until("the download to finish elsewhere", |h| completed(h) == 1);
    let row = h.rows()[0].clone();
    assert!(
        std::path::Path::new(&row.path).starts_with(&elsewhere),
        "{}",
        row.path
    );
    assert_eq!(std::fs::read(&row.path).unwrap(), payload());
}

/// A failed row's status is a plain reason; the engine's words are kept for hover.
#[test]
fn a_cut_connection_is_said_plainly() {
    let world = world();
    let mut h = world.harness_with(Settings {
        attempts: 1,
        ..world.settings()
    });
    h.view(|view| view.backend.add(&world.url("drops.bin")).unwrap());
    h.until("the cut download to fail", failed);
    let row = h.rows()[0].clone();
    let status = dm_desktop_base::rows::status_of(&row);
    assert_eq!(
        status, "Error: the connection dropped before the file was complete. Try again",
        "{:?}",
        row.state
    );
    assert!(dm_desktop_base::rows::failure_as_said(&row).is_some());
    // In the list's row and in Properties.
    h.until_shown(&status);
    h.select("drops.bin");
    h.run(Command::Properties);
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.until("it in Properties too", |h| {
        h.texts().iter().filter(|t| **t == status).count() >= 2
    });
}

/// While a range waits to be tried again, the row says which attempt it is on.
#[test]
fn a_retry_says_its_attempt() {
    let world = world();
    let mut h = world.harness_with(Settings {
        attempts: 5,
        ..world.settings()
    });
    h.view(|view| view.backend.add(&world.url("drops.bin")).unwrap());
    // With when it tries next, in the words themselves (gaps row U7).
    h.until("the row to say it retries", |h| {
        h.rows().first().is_some_and(|r| {
            dm_desktop_base::rows::status_of(r).starts_with("Connection dropped (retry 2 of 5)")
        })
    });
    // It says when it tries again, not only that it will.
    h.until("when it tries again", |h| {
        h.texts().iter().any(|t| t.contains("next try in"))
    });
    // A new address is taken while it retries, and finishes it.
    let id = h.rows()[0].id;
    h.select("drops.bin");
    assert!(h.enabled(Command::UpdateLink));
    let good = world.url("file.zip");
    h.view(|view| view.backend.set_url(id, &good).unwrap());
    h.until("the new address to finish it", |h| {
        h.rows()
            .first()
            .is_some_and(|r| r.state == State::Completed && r.url == good)
    });
}

/// A hand-off of an address already listed is added, and said.
#[test]
fn a_hand_off_of_a_listed_address_is_said_to_be_added_again() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| h.rows().len() == 1);
    // As the inbox adds a browser's hand-off: to the queue, not by the window.
    h.view(|view| {
        view.backend
            .add_with(&world.url("file.zip"), dm_core::AddOptions::default())
            .unwrap()
    });
    h.until("it to be said", |h| {
        h.said() == "Added file.zip again: that address was already in the list."
    });
}

/// A file added again, while its first copy is in the folder, is listed under the name it will
/// be placed under, not the one it will not.
#[test]
fn a_file_added_again_is_listed_under_the_name_it_will_get() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the first copy to finish", |h| completed(h) == 1);
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until_shown("file (1).zip");
    // And the control protocol says so too.
    let seen = h.call("ui.inspect", serde_json::json!({})).unwrap();
    let names: Vec<&str> = seen["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["name"].as_str())
        .collect();
    assert!(names.contains(&"file (1).zip"), "{names:?}");
    // And it is placed there once fetched.
    h.click("Start Queue");
    h.until("the second copy to finish", |h| completed(h) == 2);
    assert!(h.rows().iter().any(|r| r.path.ends_with("file (1).zip")));
}

/// Copies of one file added for later are each named as they will be saved, not all
/// "other (1).pdf".
#[test]
fn copies_waiting_in_the_list_each_get_their_own_name() {
    let world = world();
    let mut h = world.harness();
    for _ in 0..3 {
        h.add_url(&world.url("other.pdf"), "Add for later");
    }
    h.until("three rows", |h| h.rows().len() == 3);
    for name in ["other.pdf", "other (1).pdf", "other (2).pdf"] {
        h.until_shown(name);
    }
}

/// A download refused for want of a sign-in is retried with one saved in Options afterwards,
/// as a fresh copy of the address was.
#[test]
fn retry_uses_a_sign_in_saved_since_it_failed() {
    let world = world();
    let mut h = world.harness();
    h.view(|view| view.backend.add(&world.url("locked.zip")).unwrap());
    h.until("it to be refused", failed);
    let password = password();
    h.apply(|s| {
        s.site_logins = vec![dm_shell::settings::SiteLogin {
            host: "127.0.0.1".into(),
            username: "me".into(),
            password,
        }]
    });
    let id = h.rows()[0].id;
    h.view(|view| view.backend.retry(id));
    h.until("it to complete with the saved sign-in", |h| {
        completed(h) == 1
    });
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
    h.click("OK");
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

/// A failure is announced as a finish is, saying why in plain words; it was only a sound, if
/// that was on.
#[test]
fn a_failed_download_is_announced_with_why() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("secret.zip"), "Start now");
    h.until("the refusal", failed);
    h.until("the announcement", |h| {
        !h.fake.notified.lock().unwrap().is_empty()
    });
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    let told = h.fake.notified.lock().unwrap().clone();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].starts_with("secret.zip — "), "{told:?}");
    assert!(!told[0].contains("http"), "{told:?}");
}

/// Download complete can be turned off from itself ("Don't show this window again"), and
/// offers Open with beside Open.
#[test]
fn download_complete_offers_open_with_and_not_again() {
    let world = world();
    let mut h = world.harness_with(Settings {
        show_complete: true,
        ..world.settings()
    });
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", |h| completed(h) == 1);
    h.until("Download complete", |h| h.dialog_open("Download complete"));
    assert!(h.shows("Open with…"), "{:?}", h.texts());
    // Ticking marks the box and leaves the window open; nothing is saved yet.
    h.click("Don't show this window again");
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert!(h.dialog_open("Download complete"));
    assert!(
        h.view(|view| view.backend.settings()).show_complete,
        "saved before it was closed"
    );
    // Closing the window saves it.
    h.click("Close");
    h.until("the window to close", |h| {
        !h.dialog_open("Download complete")
    });
    assert!(
        !h.view(|view| view.backend.settings()).show_complete,
        "the setting stays on"
    );
    // The next download that finishes opens nothing.
    h.add_url(&world.url("other.pdf"), "Start now");
    h.until("the second download to complete", |h| completed(h) == 2);
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    assert!(!h.dialog_open("Download complete"));
}

/// A file saved a minute ago, so the folder's reader does not take it for one still being
/// written.
fn aged(path: &std::path::Path) {
    let old = std::time::SystemTime::now() - Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(old)
        .unwrap();
}

#[test]
fn a_link_saved_in_the_watched_folder_is_added_and_its_file_put_aside() {
    let world = world();
    let mut h = world.harness();
    let watched = tempfile::tempdir().unwrap();
    let dir = watched.path();
    std::fs::write(
        dir.join("one.url"),
        format!("[InternetShortcut]\nURL={}\n", world.url("watched-one.zip")),
    )
    .unwrap();
    std::fs::write(
        dir.join("two.txt"),
        format!("{}\n", world.url("watched-two.zip")),
    )
    .unwrap();
    std::fs::write(dir.join("nothing.txt"), "no address in this\n").unwrap();
    std::fs::write(dir.join("picture.png"), b"\x89PNG not read at all").unwrap();
    for f in ["one.url", "two.txt", "nothing.txt", "picture.png"] {
        aged(&dir.join(f));
    }

    // Off until it is switched on in Options.
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert!(h.rows().is_empty(), "nothing is read while it is off");
    let folder = dir.display().to_string();
    h.apply(|s| s.watch_folder = folder);

    h.until("both links to be added", |h| {
        let rows = h.rows();
        rows.iter().any(|r| r.url.ends_with("watched-one.zip"))
            && rows.iter().any(|r| r.url.ends_with("watched-two.zip"))
    });
    let done = dir.join("done");
    h.until("both put aside", |_| {
        done.join("one.url").exists() && done.join("two.txt").exists()
    });
    assert!(!dir.join("one.url").exists() && !dir.join("two.txt").exists());
    assert!(
        dir.join("nothing.txt").exists(),
        "a file with no link is left where it is"
    );
    assert!(dir.join("picture.png").exists(), "other kinds are not read");
    assert_eq!(h.rows().len(), 2, "the file with no link added nothing");
    h.until("the file with no link to be said once", |h| {
        h.view(|view| {
            view.problems
                .iter()
                .any(|p| p.contains("nothing.txt") && p.contains("no link"))
        })
    });

    // The same link saved again is already in the list: put aside, not added.
    std::fs::write(
        dir.join("again.url"),
        format!("[InternetShortcut]\nURL={}\n", world.url("watched-one.zip")),
    )
    .unwrap();
    aged(&dir.join("again.url"));
    h.until_within(
        "the link already listed to be put aside",
        Duration::from_secs(15),
        &mut |_| done.join("again.url").exists(),
    );
    assert_eq!(
        h.rows().len(),
        2,
        "a link already listed is not added again"
    );
}

/// Move/Rename of several finished files, with a pattern: `{n}` counts from 1 in the order
/// listed, `{name}` is the old name, the ending is kept, and an empty pattern leaves the names
/// alone.
#[test]
fn several_finished_files_are_named_by_a_pattern_and_keep_their_endings() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("other.pdf"), "Start now");
    h.until("both to complete", |h| completed(h) == 2);
    let (zip, pdf) = (h.row("file.zip").id, h.row("other.pdf").id);
    // In the order the list shows them.
    let listed: Vec<String> = h.call("ui.inspect", serde_json::json!({})).unwrap()["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["name"].as_str().map(str::to_string))
        .collect();
    h.run(Command::SelectAll);
    h.run(Command::MoveRename);
    h.until("Move/Rename", |h| h.dialog_open("Move/Rename"));
    assert!(
        h.shows("Name pattern"),
        "several files are named by a pattern: {:?}",
        h.texts()
    );
    h.click("OK");
    h.until("it to close", |h| !h.dialog_open("Move/Rename"));
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert_eq!(h.row("file.zip").id, zip, "empty keeps names");

    h.run(Command::MoveRename);
    h.until("Move/Rename", |h| h.dialog_open("Move/Rename"));
    h.type_text("Trip {n} ({name})");
    h.click("OK");
    let now = |h: &mut Harness, id| h.rows().into_iter().find(|r| r.id == id).unwrap();
    h.until("the new names", |h| {
        now(h, zip).filename.starts_with("Trip") && now(h, pdf).filename.starts_with("Trip")
    });
    let (zip_row, pdf_row) = (now(&mut h, zip), now(&mut h, pdf));
    let wanted = |nth: usize| {
        let (stem, ending) = listed[nth].rsplit_once('.').unwrap();
        format!("Trip {} ({stem}).{ending}", nth + 1)
    };
    let mut names = vec![zip_row.filename.clone(), pdf_row.filename.clone()];
    names.sort();
    let mut expected = vec![wanted(0), wanted(1)];
    expected.sort();
    assert_eq!(names, expected);
    assert!(
        std::path::Path::new(&zip_row.path).exists()
            && std::path::Path::new(&pdf_row.path).exists(),
        "the files on disk were renamed with them"
    );
}

/// Properties of a finished file shows its SHA-256, worked out beside the window, and says
/// whether a digest it was given matches.
#[test]
fn properties_show_the_files_sha256_and_check_a_given_digest() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the file to complete", |h| completed(h) == 1);
    let finished = h.row("file.zip");
    let expected = world
        .runtime
        .block_on(dm_core::digest(
            std::path::Path::new(&finished.path),
            dm_core::Algorithm::Sha256,
        ))
        .unwrap();
    h.select("file.zip");
    h.run(Command::Properties);
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.until_shown(&expected);
    // A matching digest, given in any case.
    h.click_beside("Check against");
    h.type_text(&expected.to_uppercase());
    h.until_shown("It matches.");
    assert!(!h.shows("It does not match."));
}

#[test]
fn a_caught_download_of_a_listed_address_asks_before_fetching_again() {
    let world = world();
    let mut h = world.harness();
    let url = world.url("file.zip");
    h.view(|view| {
        view.backend
            .add_with(&url, dm_core::AddOptions::default())
            .unwrap()
    });
    h.until("the row", |h| h.rows().len() == 1);
    h.hand_over(dm_shell::host::Request {
        url: url.clone(),
        referrer: "https://e.com/page".into(),
        caught: true,
        ..Default::default()
    });
    h.until_shown("Already in the list");
    assert_eq!(h.rows().len(), 1, "fetched again unasked");
}

/// Escape closes Download complete as its Close does, and a ticked "Don't show this window
/// again" is kept either way.
#[test]
fn escape_keeps_a_ticked_dont_show_again_as_close_does() {
    let world = world();
    let mut h = world.harness_with(Settings {
        show_complete: true,
        ..world.settings()
    });
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", |h| completed(h) == 1);
    h.until("Download complete", |h| h.dialog_open("Download complete"));
    h.click("Don't show this window again");
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert!(h.view(|view| view.backend.settings()).show_complete);
    h.key("escape", &[]);
    h.until("the window to close", |h| {
        !h.dialog_open("Download complete")
    });
    assert!(!h.view(|view| view.backend.settings()).show_complete);
}

/// A download handed over and waiting for an answer has left the browser: a window comes
/// forward for it. One that starts at once does not take the person's screen.
#[test]
fn a_hand_over_that_asks_brings_the_window_forward_and_one_that_starts_does_not() {
    let world = world();
    let mut h = world.harness();
    let handed = |h: &mut Harness, name: &str| {
        h.hand_over(dm_shell::host::Request {
            url: world.url(name),
            referrer: "https://e.com/page".into(),
            caught: true,
            ..Default::default()
        });
    };
    handed(&mut h, "quiet.zip");
    h.until("it to start", |h| h.rows().len() == 1);
    // (Its address is served nothing: the window that says so is not this test's.)
    assert!(
        !h.dialog_open("Download details"),
        "raised for a download that started"
    );
    h.apply(|s| s.ask_caught_downloads = true);
    handed(&mut h, "asks.zip");
    // Its own window, which the system puts in front as it opens (GPUI's windows open
    // focused); the test platform does not stack windows, so that part is the system's.
    h.until("Download details", |h| h.dialog_open("Download details"));
}

/// A download of known size that has no rate to go on says "Calculating…" in Time left, not
/// nothing.
#[test]
fn time_left_says_calculating_until_there_is_a_rate() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("to run", running);
    h.until_shown("Calculating…");
}

/// A double-click opens a finished file, as a file manager's does.
#[test]
fn a_new_install_opens_a_finished_file_on_a_double_click() {
    assert_eq!(Settings::default().double_click, "open");
}

/// Taking a finished download off the list asks nothing, offers Undo, and Undo puts it back
/// where it was; its file never moves.
#[test]
fn removing_a_finished_download_is_undone_from_the_status_bar() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("other.pdf"), "Start now");
    h.until("both to complete", |h| completed(h) == 2);
    let first = h.row("file.zip").id;
    let path = h.row("file.zip").path;
    let order: Vec<_> = h.rows().iter().map(|r| r.id).collect();
    h.select("file.zip");
    h.run(Command::Remove);
    h.until("the row to go", |h| h.rows().len() == 1);
    h.until("Undo", |h| {
        h.texts().iter().any(|t| t.contains("from the list"))
    });
    h.click("Undo");
    h.until("the row to come back", |h| h.rows().len() == 2);
    let ids: Vec<_> = h.rows().iter().map(|r| r.id).collect();
    assert_eq!(ids, order, "not back in its place");
    assert!(ids.contains(&first));
    assert!(std::path::Path::new(&path).is_file());
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert!(!h.shows("Undo"), "Undo stayed after it was used");
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
    h.click("OK");
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

/// A finished row says when it finished, beside "Complete", so a file from last week is found
/// by its day.
#[test]
fn a_finished_row_says_when_it_finished() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("it to finish", |h| completed(h) == 1);
    let finished = h.row("file.zip").finished_at;
    let when = dm_desktop_base::format::when(finished);
    assert!(!when.is_empty(), "a finished row has no finishing time");
    h.until_shown(&when);
    assert!(h.shows("Complete"), "Complete is no longer on its own");
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
    // The read keeps its own copy of the rows: the size is told to the next ones.
    h.view(|view| {
        for row in &mut view.rows {
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
        view.said = None;
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

/// The progress window of a download that finishes goes with it, whether or not the complete
/// window is on (it is not, by default): left open it said "Complete" with no button to press,
/// one more for each download after it.
#[test]
fn the_progress_window_of_a_finished_download_closes() {
    let world = world();
    let mut h = world.harness();
    assert!(
        !world.settings().show_complete,
        "this checks the default, where there is no complete window"
    );
    h.view(|view| view.backend.add(&world.url("stalled.bin")).unwrap());
    h.until("it to run", running);
    let id = h.rows()[0].id;
    h.update(|view, cx| view.show_progress(id, cx));
    h.until("its progress window", |h| h.dialogs().len() == 1);
    // A fresh address that finishes it.
    h.view(|view| view.backend.set_url(id, &world.url("file.zip")).unwrap());
    h.until("it to finish", |h| completed(h) == 1);
    // (A new address can ask about the name its server sent: that question is not this
    // test's.)
    h.until("its progress window to close", |h| {
        h.dialogs().iter().all(|d| d == "Name sent by the server")
    });
}

/// A download that failed is counted above the list, with one button to try every failed one
/// again; hidden, the bar stays away until another fails.
#[test]
fn failed_downloads_are_counted_above_the_list_with_retry_all() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("secret.zip"), "Start now");
    h.until("the failure", failed);
    h.until_shown("1 download didn't finish.");
    let asked = SECRET_ASKED.load(std::sync::atomic::Ordering::SeqCst);
    h.click("Retry all");
    // Tried again: asked of the server once more, and failed again, as that server still
    // wants a sign-in.
    h.until("the retry to reach the server", |_| {
        SECRET_ASKED.load(std::sync::atomic::Ordering::SeqCst) > asked
    });
    h.until("the second failure", failed);
    h.until_shown("1 download didn't finish.");
    h.click("Hide");
    h.until("the bar to go", |h| !h.shows("1 download didn't finish."));
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
    let chosen = h.view(|view| view.selected.clone());
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
            h.view(|view| view.selected.clone()),
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
    h.cx.write_to_clipboard(gpui::ClipboardItem::new_string(first.clone()));
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(&format!("{first}{second}"));
    h.click("OK");
    h.until("the refusal", |h| {
        h.texts().iter().any(|t| t.contains("looks like two links"))
    });
    assert!(!h.dialog_open("Download details"));
    assert!(h.rows().is_empty());
    // The address keeps the keyboard, chosen: what is typed next replaces it.
    h.type_text(&second);
    h.click("OK");
    h.until("Download details", |h| h.dialog_open("Download details"));
    assert!(h.shows(&second), "{:?}", h.texts());
    assert!(!h.shows(&format!("{first}{second}")));
}

/// The limiter's field takes what is typed in place of what it held; the toolbar's buttons
/// stay put when Resume becomes Retry; the progress window says what a refusal leaves
/// unknown. (Where the egui window kept its progress window and the empty list's words is
/// egui's own layout, not checked here.)
#[test]
fn the_small_things_of_round_eight() {
    let world = world();
    // The limiter: 500 typed over 64 is 500, not 64500.
    let mut h = world.harness_with(Settings {
        speed_limit: 64,
        show_progress: true,
        ..world.settings()
    });
    h.run(Command::SpeedLimiterSettings);
    h.until("the limiter", |h| h.dialogs().len() == 1);
    h.type_text("500");
    h.key("enter", &[]);
    h.until("the limit", |h| {
        h.view(|view| view.backend.settings()).speed_limit == 500
    });
    h.apply(|s| s.speed_limit = 0);

    // The toolbar: one refused, one finished.
    h.add_url(&world.url("secret.zip"), "Start now");
    h.until("the refusal", failed);
    // The server answered, with a refusal: said as one.
    h.until_shown("No — the server refused it");
    h.apply(|s| s.show_progress = false);
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
