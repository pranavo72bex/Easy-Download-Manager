// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Download details, the progress window, the questions, Download links,
//! the welcome and About, driven by the words on screen against a real
//! queue on a loopback server. Where a dialog only reads the list, the
//! rows it reads are set as a test needs them (a failed torrent, a
//! download whose every piece is in), as the list would show them.

#[path = "window/support.rs"]
#[allow(dead_code, unused_imports)]
mod support;

use egui::accesskit::Role;
use support::*;

/// The world every test here runs in, with the bar that offers the browser
/// extension put away for good: a test here hands a download over from a
/// browser, which takes the bar away under the others' clicks.
fn world() -> World {
    let world = support::world();
    let hidden = dm_shell::settings::home().join("browser-hint.hidden");
    if !hidden.exists() {
        std::fs::write(hidden, "").unwrap();
    }
    world
}

/// What the status bar says.
fn said(harness: &Harness<'_, DmApp>) -> String {
    harness.state().said.shown()
}

/// Rows added for later, one for each name, as the list now reads them.
fn later_rows(
    harness: &mut Harness<'_, DmApp>,
    world: &World,
    names: &[&str],
) -> Vec<dm_core::Entry> {
    for name in names {
        add_named(harness, world, name);
    }
    harness.state_mut().refresh();
    names.iter().map(|name| row(harness, name)).collect()
}

/// The list as the window shows it: these rows, until it is read again.
fn show_rows(harness: &mut Harness<'_, DmApp>, rows: &[dm_core::Entry]) {
    harness.state_mut().rows = rows.iter().cloned().map(Arc::new).collect();
}

fn shown(harness: &Harness<'_, DmApp>, words: &str) -> bool {
    harness.query_by_label(words).is_some()
}

fn shown_containing(harness: &Harness<'_, DmApp>, words: &str) -> bool {
    harness.query_all_by_label_contains(words).next().is_some()
}

/// Clicks what has that name once it stays where it is: a dialog whose
/// words changed sizes itself again over a frame or two, and a click
/// aimed where it was landed beside it.
fn click_settled(harness: &mut Harness<'_, DmApp>, name: &str) {
    let mut last = None;
    for _ in 0..30 {
        harness.run_steps(1);
        let now = bounds(harness, name);
        if last == Some(now) {
            break;
        }
        last = Some(now);
    }
    click(harness, name);
}

/// A slot already answered, so the dialog asks no server.
fn answered<T>(value: T) -> Option<Arc<Mutex<Option<T>>>> {
    Some(Arc::new(Mutex::new(Some(value))))
}

/* ------------------------------------------------------ Download details */

#[test]
fn a_page_found_to_list_videos_opens_the_playlist_instead() {
    let world = world();
    let mut harness = world.harness();
    let page = world.url("page.html");
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![page.clone()]);
    let playlist = dm_core::youtube::Playlist {
        id: "talks".into(),
        title: "Two talks".into(),
        cut_short: false,
        entries: vec![dm_core::youtube::PlaylistEntry {
            id: "a".into(),
            title: "First talk".into(),
            seconds: Some(60),
            page: Some(world.url("first")),
        }],
    };
    *harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .unwrap()
        .playlist
        .lock()
        .unwrap() = Some(playlist);
    harness.run_steps(3);
    assert!(harness.state().dialogs.file_info.is_none());
    let opened = harness.state().dialogs.playlist.as_ref().unwrap();
    assert_eq!(opened.url, page);
}

/// Download details over a video page whose qualities are known.
fn video_details(
    harness: &mut Harness<'_, DmApp>,
    url: &str,
    list: Vec<dm_core::videopage::Choice>,
) {
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![url.to_string()]);
    let d = harness.state_mut().dialogs.file_info.as_mut().unwrap();
    d.probed = answered(dm_desktop::dialogs::Probed {
        page: true,
        ..Default::default()
    });
    d.choices = answered(list);
    harness.run_steps(3);
}

#[test]
fn a_videos_quality_is_picked_by_its_chip_and_its_size_waits_for_the_start() {
    let world = world();
    let mut harness = world.harness();
    let choice = |quality: &str, label: &str, size| dm_core::videopage::Choice {
        quality: quality.into(),
        label: label.into(),
        size,
    };
    video_details(
        &mut harness,
        &world.url("watch"),
        vec![
            choice("1080/mp4", "MP4 1920×1080", Some(275_000_000)),
            choice("720/mp4", "MP4 1280×720", None),
            choice("audio/m4a", "M4A audio", Some(4_000_000)),
        ],
    );
    assert!(shown(&harness, "Known once the video starts"));
    let chips: Vec<String> = harness
        .query_all_by_role(Role::Button)
        .filter_map(|n| n.accesskit_node().label())
        .filter(|l| l.contains("p MP4"))
        .collect();
    assert_eq!(chips.len(), 2, "{chips:?}");
    harness
        .query_all_by_label_contains("720p")
        .last()
        .unwrap()
        .click();
    harness.run_steps(3);
    let d = harness.state().dialogs.file_info.as_ref().unwrap();
    assert_eq!(d.quality.as_deref(), Some("720/mp4"));
    assert_eq!(d.label.as_deref(), Some("MP4 1280×720"));
}

#[test]
fn a_video_page_with_no_qualities_says_so() {
    let world = world();
    let mut harness = world.harness();
    video_details(&mut harness, &world.url("watch"), Vec::new());
    assert!(shown(&harness, "—"), "an empty list is a dash");
    assert!(!shown(&harness, "Reading the video's qualities…"));
}

#[test]
fn a_link_at_a_moment_offers_that_moment_and_a_part_is_shaped_and_checked() {
    let world = world();
    let mut harness = world.harness();
    video_details(
        &mut harness,
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=34",
        Vec::new(),
    );
    assert!(shown(&harness, "This link starts at 0:34."));
    click(&mut harness, "From 0:34");
    {
        let d = harness.state().dialogs.file_info.as_ref().unwrap();
        assert!(d.part);
        assert_eq!(d.part_from, "0:34");
    }
    click(&mut harness, "Square 1:1");
    assert_eq!(
        harness.state().dialogs.file_info.as_ref().unwrap().shape,
        Some(dm_core::shape::Shape::Square)
    );
    if dm_core::gif::possible_for("https://www.youtube.com/watch?v=dQw4w9WgXcQ") {
        click(&mut harness, "GIF");
        assert!(harness.state().dialogs.file_info.as_ref().unwrap().gif);
        click(&mut harness, "Video");
        assert!(!harness.state().dialogs.file_info.as_ref().unwrap().gif);
    }
    // An end before the start keeps the dialog and says how to write one.
    harness
        .state_mut()
        .dialogs
        .file_info
        .as_mut()
        .unwrap()
        .part_to = "0:10".into();
    harness.run_steps(2);
    click(&mut harness, "Start now");
    let d = harness.state().dialogs.file_info.as_ref().expect("closed");
    assert!(d.says.starts_with("Enter the start and end"), "{}", d.says);
    assert!(harness.state().rows().is_empty());
}

#[test]
fn a_caught_download_says_its_browser_and_can_be_saved_here_from_now_on() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.ask_caught_downloads = true;
    harness
        .state_mut()
        .dialogs
        .open_handed(dm_shell::host::Request {
            url: world.url("other.pdf"),
            browser: Some("Firefox".into()),
            caught: true,
            ..Default::default()
        });
    harness.run_steps(3);
    settle_details(&mut harness);
    assert!(shown(&harness, "From Firefox"));
    click(&mut harness, "Always save here without asking");
    click(&mut harness, "Start now");
    harness.state_mut().refresh();
    assert!(harness.state().dialogs.file_info.is_none());
    assert_eq!(harness.state().rows().len(), 1);
    assert!(!harness.state().applied.ask_caught_downloads);
}

#[test]
fn the_category_drop_down_puts_the_file_in_a_kinds_folder_or_back_to_its_own() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("other.pdf")]);
    harness.run_steps(3);
    settle_details(&mut harness);
    harness
        .get_by_role_and_label(Role::ComboBox, "Category")
        .click();
    harness.run_steps(3);
    harness
        .query_all_by_label("Documents")
        .last()
        .unwrap()
        .click();
    harness.run_steps(3);
    let mut sorted = harness.state().applied.categories();
    sorted.enabled = true;
    let documents = sorted
        .destination(
            &harness.state().applied.download_dir,
            dm_core::Category::Documents,
        )
        .display()
        .to_string();
    assert_eq!(
        harness.state().dialogs.file_info.as_ref().unwrap().folder,
        documents
    );
    harness
        .get_by_role_and_label(Role::ComboBox, "Category")
        .click();
    harness.run_steps(3);
    harness
        .query_all_by_label("As its type says")
        .last()
        .unwrap()
        .click();
    harness.run_steps(3);
    assert!(harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .unwrap()
        .folder
        .is_empty());
    // The queue it waits in, from the queues there are.
    harness.get_by_role_and_label(Role::ComboBox, "to").click();
    harness.run_steps(3);
    harness
        .query_all_by_label("Main queue")
        .last()
        .unwrap()
        .click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().dialogs.file_info.as_ref().unwrap().queue,
        dm_core::MAIN_QUEUE
    );
}

#[test]
fn an_address_that_cannot_be_added_is_said_in_the_dialog() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec!["http://".to_string()]);
    harness.run_steps(3);
    settle_details(&mut harness);
    click(&mut harness, "Start now");
    let d = harness.state().dialogs.file_info.as_ref().expect("closed");
    assert!(d.says.contains("names no site"), "{}", d.says);
    assert!(shown_containing(&harness, "names no site"));
}

#[test]
fn begun_while_asking_only_a_single_plain_file_and_a_refusal_is_said() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.start_while_asking = true;
    // Several addresses wait for the person.
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("a.bin"), world.url("b.bin")]);
    harness.run_steps(2);
    harness.state_mut().refresh();
    assert!(harness.state().rows().is_empty());
    assert!(
        harness
            .state()
            .dialogs
            .file_info
            .as_ref()
            .unwrap()
            .early_tried
    );
    // One that cannot be added is said, and nothing begins.
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec!["http://".to_string()]);
    harness.run_steps(2);
    assert!(
        said(&harness).contains("names no site"),
        "{}",
        said(&harness)
    );
    assert!(harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .unwrap()
        .early
        .is_none());
}

#[test]
fn a_handed_download_begun_while_asking_is_placed_not_added_again() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.start_while_asking = true;
    harness
        .state_mut()
        .dialogs
        .open_handed(dm_shell::host::Request {
            url: world.url("other.pdf"),
            ..Default::default()
        });
    harness.run_steps(3);
    let early = harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .unwrap()
        .early
        .expect("begun at once");
    settle_details(&mut harness);
    click(&mut harness, "Start now");
    harness.state_mut().refresh();
    assert!(harness.state().dialogs.file_info.is_none());
    let rows = harness.state().rows();
    assert_eq!(rows.len(), 1, "added again");
    assert_eq!(rows[0].id, early);
}

/* ------------------------------------------------------- progress window */

#[test]
fn a_progress_window_whose_row_is_gone_closes() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().dialogs.progress.insert(987_654);
    harness.run_steps(2);
    assert!(harness.state().dialogs.progress.is_empty());
}

/// Whether `look` finds what it looks for over these rows. The window reads
/// the list again a quarter of a second after it last did, which puts the
/// queue's own rows back: they are shown again before each frame, and a
/// frame that read the list is looked past.
fn sees(
    harness: &mut Harness<'_, DmApp>,
    rows: &[dm_core::Entry],
    look: impl Fn(&Harness<'_, DmApp>) -> bool,
) -> bool {
    (0..40).any(|_| {
        show_rows(harness, rows);
        harness.step();
        look(harness)
    })
}

fn sees_words(harness: &mut Harness<'_, DmApp>, rows: &[dm_core::Entry], words: &str) -> bool {
    sees(harness, rows, |h| shown(h, words))
}

/// Clicks `name` over these rows until `done` says it took (see [`sees`]).
fn press_over(
    harness: &mut Harness<'_, DmApp>,
    rows: &[dm_core::Entry],
    name: &str,
    done: impl Fn(&DmApp) -> bool,
) {
    for _ in 0..20 {
        show_rows(harness, rows);
        harness.step();
        if let Some(node) = harness.query_all_by_label(name).last() {
            node.click();
        }
        for _ in 0..2 {
            show_rows(harness, rows);
            harness.step();
        }
        if done(harness.state()) {
            return;
        }
    }
    panic!("{name:?} did not take");
}

/// Opens the progress window of `row`, as the list shows it.
fn progress_of(harness: &mut Harness<'_, DmApp>, row: &dm_core::Entry) {
    show_rows(harness, std::slice::from_ref(row));
    harness.state_mut().dialogs.progress.insert(row.id);
    harness.run_steps(3);
}

#[test]
fn whether_a_download_can_go_on_is_said_for_torrents_and_failures() {
    let world = world();
    let mut harness = world.harness();
    let rows = later_rows(&mut harness, &world, &["a.torrent", "plain.bin"]);
    let (torrent, plain) = (&rows[0], &rows[1]);

    let mut failed = torrent.clone();
    failed.state = State::Failed("the file could not be read".into());
    let failed = [failed];
    progress_of(&mut harness, &failed[0]);
    assert!(sees_words(
        &mut harness,
        &failed,
        "No — nothing of it was fetched"
    ));
    // Failed, it is tried again rather than resumed.
    assert!(sees_words(&mut harness, &failed, "Retry"));
    harness.state_mut().dialogs.progress.clear();
    harness.run_steps(2);

    let mut paused = torrent.clone();
    paused.state = State::Paused;
    let paused = [paused];
    progress_of(&mut harness, &paused[0]);
    assert!(sees_words(
        &mut harness,
        &paused,
        "Yes, from the pieces it already has"
    ));
    harness.state_mut().dialogs.progress.clear();
    harness.run_steps(2);

    let mut failed = plain.clone();
    failed.state = State::Failed("could not connect".into());
    let failed = [failed];
    progress_of(&mut harness, &failed[0]);
    assert!(sees_words(
        &mut harness,
        &failed,
        "Not known: it failed before the server answered"
    ));
}

#[test]
fn a_download_held_by_the_schedule_asks_before_starting_from_its_window() {
    let world = world();
    let mut harness = world.harness();
    let mut held = later_rows(&mut harness, &world, &["held.bin"]).remove(0);
    held.state = State::Paused;
    held.paused_by_schedule = true;
    let held = [held];
    progress_of(&mut harness, &held[0]);
    press_over(&mut harness, &held, "Start now", |app| {
        app.dialogs.confirm_outside.is_some()
    });
    assert_eq!(
        harness.state().dialogs.confirm_outside.as_deref(),
        Some(&[held[0].id][..])
    );
}

#[test]
fn every_piece_in_is_said_while_the_parts_are_joined() {
    let world = world();
    let mut harness = world.harness();
    let mut joining = later_rows(&mut harness, &world, &["joined.bin"]).remove(0);
    joining.state = State::Running;
    joining.progress.total = Some(1000);
    joining.progress.downloaded = 1000;
    let joining = [joining];
    progress_of(&mut harness, &joining[0]);
    click_settled(&mut harness, "Connections");
    assert!(sees_words(
        &mut harness,
        &joining,
        "Every piece is in; putting them together."
    ));
}

#[test]
fn a_pieces_line_says_what_came_where_no_size_is_known_or_that_it_waits() {
    let world = world();
    let mut settings = world.settings();
    settings.connections = 4;
    let mut harness = world.harness_with(settings);
    add_url(&mut harness, &world.url("ranges-stall.bin"), "Start now");
    until_within(
        &mut harness,
        "several pieces under way",
        Duration::from_secs(20),
        |app| {
            app.backend
                .list()
                .first()
                .is_some_and(|r| app.backend.connections(r.id).len() > 1)
        },
    );
    harness.state_mut().refresh();
    let running = harness.state().rows()[0].as_ref().clone();
    // Of a length nobody said: what came, not a bar (and not "waiting",
    // which is said only while it runs).
    let mut no_size = running.clone();
    no_size.progress.total = None;
    no_size.state = State::Paused;
    let no_size = [no_size];
    progress_of(&mut harness, &no_size[0]);
    click_settled(&mut harness, "Connections");
    assert!(
        sees(&mut harness, &no_size, |h| shown_containing(h, "so far")),
        "no piece said what came"
    );
    // Running, a piece no request is open for yet waits.
    assert!(
        sees(&mut harness, std::slice::from_ref(&running), |h| {
            shown_containing(h, "waiting, ")
        }),
        "no piece waits"
    );
    harness.state().backend().cancel(running.id);
}

/* ------------------------------------------------------------ questions */

/// The Remove question over these rows.
fn ask_remove(harness: &mut Harness<'_, DmApp>, rows: &[dm_core::Entry]) {
    harness.state_mut().dialogs.confirm_remove = None;
    harness.state_mut().dialogs.remove_files = false;
    show_rows(harness, rows);
    harness.state_mut().dialogs.confirm_remove = Some(rows.iter().map(|r| r.id).collect());
    harness.run_steps(3);
}

#[test]
fn removing_says_what_happens_to_unfinished_downloads_in_their_number() {
    let world = world();
    let mut harness = world.harness();
    let rows = later_rows(&mut harness, &world, &["a.bin", "b.bin"]);
    let fetched: Vec<_> = rows
        .iter()
        .map(|r| {
            let mut r = r.clone();
            r.state = State::Paused;
            r.progress.downloaded = 10;
            r
        })
        .collect();
    ask_remove(&mut harness, &fetched[..1]);
    assert!(sees_words(
        &mut harness,
        &fetched[..1],
        "This download isn't finished. Removing it discards what was downloaded."
    ));
    ask_remove(&mut harness, &fetched);
    assert!(sees_words(
        &mut harness,
        &fetched,
        "Some downloads aren't finished. Removing them discards what was downloaded."
    ));
    assert!(shown(&harness, "Downloads: 2"));
    press_over(&mut harness, &fetched, "Keep them", |app| {
        app.dialogs.confirm_remove.is_none()
    });
    assert_eq!(harness.state().backend().list().len(), 2, "removed anyway");
}

#[test]
fn removing_finished_downloads_says_whether_their_files_stay() {
    let world = world();
    let mut harness = world.harness();
    let rows = later_rows(&mut harness, &world, &["c.bin", "d.bin", "e.bin"]);
    let files = world.dir.path().join("finished");
    std::fs::create_dir_all(&files).unwrap();
    let done: Vec<_> = rows
        .iter()
        .map(|r| {
            let mut r = r.clone();
            r.state = State::Completed;
            r.path = files
                .join(dm_desktop::app::name_of(&r))
                .display()
                .to_string();
            r
        })
        .collect();
    // Files gone from the disk: nothing to keep.
    ask_remove(&mut harness, &done[..1]);
    assert!(sees_words(
        &mut harness,
        &done[..1],
        "This removes it from the list."
    ));
    ask_remove(&mut harness, &done[..2]);
    assert!(sees_words(
        &mut harness,
        &done[..2],
        "This removes them from the list."
    ));
    assert!(sees_words(&mut harness, &done[..2], "Remove them"));
    // On the disk: kept unless asked.
    for r in &done {
        std::fs::write(&r.path, b"x").unwrap();
    }
    ask_remove(&mut harness, &done[..1]);
    assert!(sees_words(&mut harness, &done[..1], "Its file is kept."));
    ask_remove(&mut harness, &done[..2]);
    assert!(sees_words(
        &mut harness,
        &done[..2],
        "Their files are kept."
    ));
    press_over(&mut harness, &done[..2], "Also delete the files", |app| {
        app.dialogs.remove_files
    });
    assert!(sees_words(
        &mut harness,
        &done[..2],
        "Their files are deleted too."
    ));
    assert!(sees_words(&mut harness, &done[..2], "Yes, delete"));
    // Beside an unfinished one, the finished ones are said apart.
    let mut mixed = done.clone();
    mixed[2].state = State::Paused;
    ask_remove(&mut harness, &mixed);
    assert!(sees_words(
        &mut harness,
        &mixed,
        "Some downloads aren't finished. They lose what was downloaded."
    ));
    assert!(sees_words(
        &mut harness,
        &mixed,
        "The finished files are kept."
    ));
    press_over(&mut harness, &mixed, "Also delete the files", |app| {
        app.dialogs.remove_files
    });
    assert!(sees_words(
        &mut harness,
        &mixed,
        "The finished files are deleted too."
    ));
}

#[cfg(unix)]
#[test]
fn a_finished_file_that_cannot_be_deleted_with_its_row_is_said() {
    use std::os::unix::fs::PermissionsExt;
    let world = world();
    let mut harness = world.harness();
    let mut done = later_rows(&mut harness, &world, &["kept.bin"]).remove(0);
    let locked = world.dir.path().join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    let file = locked.join("kept.bin");
    std::fs::write(&file, b"x").unwrap();
    done.state = State::Completed;
    done.path = file.display().to_string();
    let done = [done];
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    ask_remove(&mut harness, &done);
    press_over(&mut harness, &done, "Also delete the file", |app| {
        app.dialogs.remove_files
    });
    press_over(&mut harness, &done, "Yes, delete", |app| {
        app.dialogs.confirm_remove.is_none()
    });
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(file.is_file());
    assert!(
        said(&harness).starts_with("Could not delete"),
        "{}",
        said(&harness)
    );
    assert!(
        harness.state().backend().list().is_empty(),
        "the row stayed"
    );
}

#[test]
fn cancelling_a_download_that_fetched_some_says_it_is_lost_and_one_gone_is_not_asked() {
    let world = world();
    let mut harness = world.harness();
    let mut running = later_rows(&mut harness, &world, &["f.bin"]).remove(0);
    running.state = State::Paused;
    running.progress.downloaded = 100;
    let fetched = [running.clone()];
    show_rows(&mut harness, &fetched);
    harness.state_mut().dialogs.confirm_cancel = Some(running.id);
    assert!(sees_words(
        &mut harness,
        &fetched,
        "Cancelling discards what was downloaded."
    ));
    press_over(&mut harness, &fetched, "Keep downloading", |app| {
        app.dialogs.confirm_cancel.is_none()
    });
    // Finished meanwhile: nothing to cancel, nothing asked.
    running.state = State::Completed;
    harness.state_mut().dialogs.confirm_cancel = Some(running.id);
    assert!(sees(&mut harness, &[running], |h| h
        .state()
        .dialogs
        .confirm_cancel
        .is_none()));
}

#[test]
fn starting_several_outside_the_schedule_counts_them_and_opens_their_windows() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_progress = true;
    let rows = later_rows(&mut harness, &world, &["g.bin", "h.bin"]);
    harness.state_mut().dialogs.confirm_outside = Some(rows.iter().map(|r| r.id).collect());
    harness.run_steps(3);
    assert!(shown(
        &harness,
        "2 downloads wait for the schedule or the download limit."
    ));
    click(&mut harness, "Start now");
    assert!(harness.state().dialogs.confirm_outside.is_none());
    for r in &rows {
        assert!(harness.state().dialogs.progress.contains(&r.id));
    }
}

#[test]
fn exiting_with_one_running_and_more_waiting_counts_both() {
    let world = world();
    let mut harness = world.harness();
    let rows = later_rows(&mut harness, &world, &["i.bin", "j.bin", "k.bin"]);
    let mut busy = rows.clone();
    busy[0].state = State::Running;
    busy[1].state = State::Queued;
    busy[2].state = State::Queued;
    busy[1].queue = String::new();
    busy[2].queue = String::new();
    show_rows(&mut harness, &busy);
    let (running, waiting) = harness.state().running_and_waiting();
    assert!(running > 0 && waiting > 0, "{running} {waiting}");
    harness.state_mut().dialogs.confirm_exit = true;
    assert!(
        sees(&mut harness, &busy, |h| {
            shown_containing(h, "more is waiting to start.")
                || shown_containing(h, "more are waiting to start.")
        }),
        "the waiting ones went uncounted"
    );
}

/* -------------------------------------------------------- Download links */

/// A page's links, handed over by the browser and asked about.
fn hand_links(harness: &mut Harness<'_, DmApp>, world: &World, links: Vec<String>) {
    harness.state_mut().applied.ask_panel_downloads = true;
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: world.url("page.html"),
            referrer: world.url("page.html"),
            links,
            ask: true,
            ..Default::default()
        });
    harness.state_mut().take_handed();
    harness.run_steps(3);
    assert!(shown(harness, "Download links"));
}

#[test]
fn download_links_unticks_all_and_one_folder_must_be_named() {
    let world = world();
    let mut harness = world.harness();
    hand_links(
        &mut harness,
        &world,
        vec![world.url("file.zip"), world.url("other.pdf")],
    );
    click(&mut harness, "Uncheck all");
    let d = harness.state().dialogs.links.as_ref().unwrap();
    assert!(d.items.iter().all(|(_, on)| !on));
    assert!(d.chosen().is_empty());
    click(&mut harness, "Check all");
    click(&mut harness, "Every file to this category");
    assert_eq!(
        harness.state().dialogs.links.as_ref().unwrap().save.save_to,
        dm_desktop::dialogs::SaveTo::OneCategory
    );
    click(&mut harness, "Each file to the folder of its category");
    assert_eq!(
        harness.state().dialogs.links.as_ref().unwrap().save.save_to,
        dm_desktop::dialogs::SaveTo::ByCategory
    );
    harness
        .query_all_by_label("Every file to this folder")
        .find(|n| n.accesskit_node().role() != Role::TextInput)
        .unwrap()
        .click();
    harness.run_steps(3);
    click(&mut harness, "Start now");
    assert_eq!(said(&harness), "Choose the folder every file goes to.");
    assert!(
        harness.state().dialogs.links.is_some(),
        "closed without a folder"
    );
    harness.state_mut().refresh();
    assert!(harness.state().rows().is_empty());
    // Later, into the queue picked.
    harness.get_by_role_and_label(Role::ComboBox, "to").click();
    harness.run_steps(3);
    harness
        .query_all_by_label("Main queue")
        .last()
        .unwrap()
        .click();
    harness.run_steps(3);
    click(&mut harness, "Cancel");
    assert!(harness.state().dialogs.links.is_none());
}

#[test]
fn a_link_edited_keeps_the_name_given_under_its_new_address() {
    let world = world();
    let mut harness = world.harness();
    let old = world.url("file.zip");
    hand_links(&mut harness, &world, vec![old.clone()]);
    {
        let d = harness.state_mut().dialogs.links.as_mut().unwrap();
        d.editing = Some(0);
        d.names.insert(old.clone(), "given.zip".into());
    }
    harness.run_steps(3);
    let new = world.url("pack.zip");
    let field = harness.get_by_role_and_label(Role::TextInput, "Address");
    field.click();
    harness.run_steps(2);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run_steps(1);
    harness
        .get_by_role_and_label(Role::TextInput, "Address")
        .type_text(&new);
    harness.run_steps(3);
    let d = harness.state().dialogs.links.as_ref().unwrap();
    assert_eq!(d.items[0].0, new);
    assert_eq!(d.names.get(&new).map(String::as_str), Some("given.zip"));
    assert!(!d.names.contains_key(&old));
}

#[test]
fn links_that_cannot_be_added_are_said() {
    let world = world();
    let mut harness = world.harness();
    hand_links(&mut harness, &world, vec!["http://".to_string()]);
    {
        // Listed and ticked, as a person ticks it again after it was refused.
        let d = harness.state_mut().dialogs.links.as_mut().unwrap();
        d.hide_html = false;
        d.refused.insert("http://".into());
        d.items[0].1 = true;
    }
    harness.run_steps(2);
    click(&mut harness, "Add for later");
    assert!(
        said(&harness).contains("names no site"),
        "{}",
        said(&harness)
    );
    // And from the clipboard's list.
    harness.state_mut().dialogs.clip_links = Some(vec![("http://".to_string(), true)]);
    harness.run_steps(3);
    click(&mut harness, "Start now");
    assert!(harness.state().dialogs.clip_links.is_none());
    assert!(
        said(&harness).contains("names no site"),
        "{}",
        said(&harness)
    );
}

/* ------------------------------------------------------ welcome and About */

#[test]
fn the_welcome_with_no_browser_found_offers_each_stores_page() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().dialogs.welcome = Some(Vec::new());
    harness.run_steps(3);
    assert!(shown(
        &harness,
        "No browser found. Here is the page for each:"
    ));
    for (button, page) in [
        ("Microsoft Edge", dm_desktop::app::EXTENSION_EDGE),
        ("Firefox", dm_desktop::app::EXTENSION_FIREFOX),
        ("Chrome and others", dm_desktop::app::EXTENSION_CHROME),
    ] {
        harness
            .query_all_by_label(button)
            .find(|n| n.accesskit_node().role() == Role::Button)
            .unwrap()
            .click();
        harness.run_steps(3);
        let fake = world.fake.clone();
        until(&mut harness, "the store's page to open", move |_| {
            fake.opened.lock().unwrap().iter().any(|o| o == page)
        });
    }
    harness.state_mut().dialogs.welcome = Some(vec![dm_shell::register::Installed {
        name: "Brave",
        store: dm_shell::register::Store::Chrome,
    }]);
    harness.run_steps(3);
    assert!(shown(&harness, "Add to Brave"));
    click(&mut harness, "Later");
    assert!(harness.state().dialogs.welcome.is_none());
}

#[test]
fn about_opens_its_pages_and_offers_a_newer_version_to_download() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().dialogs.about_open = true;
    harness.run_steps(3);
    click(&mut harness, "Privacy");
    let fake = world.fake.clone();
    until(&mut harness, "the privacy page to open", move |_| {
        fake.opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o == dm_desktop::update::PRIVACY)
    });
    let notes = world.url("notes.html");
    *dm_core::lock(&dm_desktop::update::LATEST_NOTES) = Some(notes.clone());
    *dm_core::lock(&harness.state().update) = dm_desktop::update::Update::Newer(
        "99.0.0".into(),
        Some(dm_desktop::update::Installer {
            url: world.url("file.zip"),
            sha256: "0".repeat(64),
        }),
    );
    harness.run_steps(3);
    click(&mut harness, "What's new");
    assert!(world.fake.opened.lock().unwrap().contains(&notes));
    click(&mut harness, "Download the update");
    let id = harness.state().update_row.expect("no update row");
    assert!(harness.state().backend().row(id).is_some());
    assert!(
        said(&harness).starts_with("Downloading the update."),
        "{}",
        said(&harness)
    );
}

/* ------------------------------------------------- the rows' own dialogs */

#[test]
fn a_new_address_that_is_no_link_is_said_and_cancel_closes() {
    let world = world();
    let mut harness = world.harness();
    let row = later_rows(&mut harness, &world, &["old.bin"]).remove(0);
    harness.state_mut().dialogs.update_link = Some((row.id, "not a link".into(), String::new()));
    harness.run_steps(3);
    click(&mut harness, "OK");
    let (_, _, says) = harness.state().dialogs.update_link.clone().expect("closed");
    assert!(!says.is_empty(), "nothing said");
    assert!(shown(&harness, &says));
    click(&mut harness, "Cancel");
    assert!(harness.state().dialogs.update_link.is_none());
    assert_eq!(harness.state().backend().row(row.id).unwrap().url, row.url);
}

#[test]
fn a_queue_picked_in_move_to_queue_wins_over_a_name_half_typed() {
    let world = world();
    let mut harness = world.harness();
    let row = later_rows(&mut harness, &world, &["moved.bin"]).remove(0);
    harness.state().backend().set_queue(row.id, "Night");
    harness.state_mut().refresh();
    harness.state_mut().dialogs.move_to_queue =
        Some((vec![row.id], "Night".into(), "Weekend".into()));
    harness.run_steps(3);
    harness
        .query_all_by_label("Main queue")
        .last()
        .unwrap()
        .click();
    harness.run_steps(3);
    let (_, queue, new) = harness.state().dialogs.move_to_queue.clone().unwrap();
    assert_eq!(queue, "Main queue");
    assert!(new.is_empty(), "the new name stayed");
    click(&mut harness, "OK");
    assert!(harness.state().dialogs.move_to_queue.is_none());
    assert_eq!(
        harness.state().backend().row(row.id).unwrap().queue,
        dm_core::MAIN_QUEUE
    );
}

#[test]
fn moving_or_renaming_what_is_not_finished_says_why_and_skips_what_is_gone() {
    let world = world();
    let mut harness = world.harness();
    let rows = later_rows(&mut harness, &world, &["l.bin", "m.bin"]);
    // One, renamed: refused, and said.
    harness.state_mut().dialogs.rename = Some(dm_desktop::dialogs::MoveRename {
        ids: vec![rows[0].id],
        name: "renamed.bin".into(),
        ..Default::default()
    });
    harness.run_steps(3);
    click(&mut harness, "OK");
    assert!(harness.state().dialogs.rename.is_none());
    let refused = said(&harness);
    assert!(!refused.is_empty(), "the refusal went unsaid");
    // Several, moved and named by a pattern, and one no longer listed.
    let folder = world.dir.path().join("elsewhere");
    harness.state_mut().said = Default::default();
    harness.state_mut().dialogs.rename = Some(dm_desktop::dialogs::MoveRename {
        ids: vec![987_654, rows[0].id, rows[1].id],
        folder: folder.display().to_string(),
        pattern: "part {n}".into(),
        ..Default::default()
    });
    harness.run_steps(3);
    click(&mut harness, "OK");
    assert!(harness.state().dialogs.rename.is_none());
    assert!(!said(&harness).is_empty(), "the refusal went unsaid");
    assert_eq!(harness.state().backend().list().len(), 2);
}

/* ------------------------------------------------------ series and Add URL */

#[test]
fn a_series_is_cancelled_and_its_list_unticked() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().dialogs.open_series();
    {
        let d = harness.state_mut().dialogs.series.as_mut().unwrap();
        d.template = world.url("photo*.jpg");
        d.from = "1".into();
        d.to = "3".into();
    }
    harness.run_steps(3);
    assert!(shown_containing(&harness, "3 addresses, the first"));
    click(&mut harness, "Cancel");
    assert!(harness.state().dialogs.series.is_none());
    harness.state_mut().refresh();
    assert!(harness.state().rows().is_empty());
    // The clipboard's list: Uncheck all leaves nothing to add.
    harness.state_mut().dialogs.clip_links =
        Some(vec![(world.url("a.zip"), true), (world.url("b.zip"), true)]);
    harness.run_steps(3);
    click(&mut harness, "Uncheck all");
    assert!(harness
        .state()
        .dialogs
        .clip_links
        .as_ref()
        .unwrap()
        .iter()
        .all(|(_, on)| !on));
    assert!(shown(&harness, "Found 2 links; 0 ticked."));
}

#[test]
fn add_url_offers_its_recent_addresses_to_pick_again() {
    let world = world();
    let mut harness = world.harness();
    let before = world.url("before.zip");
    harness.state_mut().applied.recent_urls = vec![before.clone()];
    harness.state_mut().dialogs.open_add_url(None);
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::ComboBox, "Recent")
        .click();
    harness.run_steps(3);
    harness
        .query_all_by_label(&before)
        .last()
        .expect("the recent address is not offered")
        .click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().dialogs.add_url.as_ref().unwrap().address,
        before
    );
}

#[test]
fn the_typed_path_for_a_list_is_cancelled() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().dialogs.list_file = Some(dm_desktop::dialogs::ListFile {
        save: true,
        ..Default::default()
    });
    harness.run_steps(3);
    assert!(shown(&harness, "No file picker here: type the path."));
    click(&mut harness, "Cancel");
    assert!(harness.state().dialogs.list_file.is_none());
}

#[test]
fn an_address_already_finished_can_replace_its_old_file() {
    let world = world();
    let mut harness = world.harness();
    let url = world.url("file.zip");
    harness.state().backend().add(&url).unwrap();
    until(&mut harness, "the first to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let first = harness.state().rows()[0].as_ref().clone();
    assert!(std::path::Path::new(&first.path).is_file());
    harness.state_mut().dialogs.duplicate = Some((url.clone(), first.id));
    harness.run_steps(3);
    click(&mut harness, "Replace the old file");
    assert!(harness.state().dialogs.duplicate.is_none());
    harness.state_mut().refresh();
    let rows = harness.state().backend().list();
    assert_eq!(rows.len(), 2, "no second row to replace it");
    let second = rows.iter().find(|r| r.id != first.id).unwrap();
    assert_eq!(
        std::path::Path::new(second.save_dir.as_deref().unwrap_or_default()),
        std::path::Path::new(&first.path).parent().unwrap()
    );
}

/* ----------------------------------------------- the Scheduler's controls */

#[test]
fn the_schedulers_days_and_stop_act_on_what_it_shows() {
    let world = world();
    let mut harness = world.harness();
    let mut settings = harness.state().applied.clone();
    settings.schedule.enabled = true;
    settings.schedule.days = Vec::new();
    harness
        .state_mut()
        .dialogs
        .open_scheduler(&settings, dm_core::MAIN_QUEUE);
    harness.run_steps(4);
    // No days kept is every day, shown ticked.
    let days = |h: &Harness<'_, DmApp>| {
        h.state()
            .dialogs
            .scheduler
            .as_ref()
            .unwrap()
            .schedule
            .days
            .clone()
    };
    assert_eq!(days(&harness), (0..=6).collect::<Vec<u8>>());
    click(&mut harness, "Mon");
    assert_eq!(days(&harness), [0, 2, 3, 4, 5, 6]);
    click(&mut harness, "Mon");
    assert_eq!(days(&harness), (0..=6).collect::<Vec<u8>>());
    click(&mut harness, "Once, on this date");
    click(&mut harness, "Every week, on the days ticked");
    assert!(!harness.state().dialogs.scheduler.as_ref().unwrap().once);
    for press in [false, true] {
        let stop = harness
            .query_all_by_label("Stop")
            .rfind(|n| n.accesskit_node().role() == Role::Button)
            .unwrap();
        if press {
            stop.click();
        } else {
            stop.scroll_to_me();
            harness.run_steps(3);
        }
    }
    harness.run_steps(3);
    let said = harness.state().said.english();
    assert!(said.contains("Main queue"), "Stop: {said:?}");
}

/* ------------------------------------------------- names and folders asked */

#[test]
fn a_name_another_download_waits_with_is_said_to_be_numbered() {
    let world = world();
    let mut harness = world.harness();
    later_rows(&mut harness, &world, &["file.zip"]);
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    harness.run_steps(3);
    settle_details(&mut harness);
    until(&mut harness, "the server's name", |app| {
        app.dialogs
            .file_info
            .as_ref()
            .is_some_and(|d| d.name == "file.zip")
    });
    assert!(shown(
        &harness,
        "A download with this name exists. This one will be numbered."
    ));
}

#[test]
fn browse_says_when_there_is_no_folder_picker_and_takes_the_one_picked() {
    let world = world();
    let mut harness = world.harness();
    hand_links(&mut harness, &world, vec![world.url("file.zip")]);
    harness
        .query_all_by_label("Every file to this folder")
        .find(|n| n.accesskit_node().role() != Role::TextInput)
        .unwrap()
        .click();
    harness.run_steps(3);
    *world.fake.no_chooser.lock().unwrap() = true;
    click(&mut harness, "Browse…");
    assert!(shown(&harness, "No folder picker here: type the path."));
    assert!(harness
        .state()
        .dialogs
        .links
        .as_ref()
        .unwrap()
        .save
        .folder
        .is_empty());
    *world.fake.no_chooser.lock().unwrap() = false;
    let picked = world.dir.path().join("picked");
    *world.fake.chosen.lock().unwrap() = Some(picked.clone());
    click(&mut harness, "Browse…");
    assert!(!shown(&harness, "No folder picker here: type the path."));
    assert_eq!(
        harness.state().dialogs.links.as_ref().unwrap().save.folder,
        picked.display().to_string()
    );
}

/* ---------------------------------------------------------------- Options */

fn open_options(harness: &mut Harness<'_, DmApp>, tab: dm_desktop::dialogs::Tab) {
    let settings = harness.state().applied.clone();
    harness.state_mut().dialogs.open_options(&settings);
    harness.state_mut().dialogs.options.as_mut().unwrap().tab = tab;
    harness.run_steps(4);
}

#[test]
fn restore_defaults_puts_the_tabs_settings_back_in_the_draft() {
    let world = world();
    let mut harness = world.harness();
    open_options(&mut harness, dm_desktop::dialogs::Tab::Downloads);
    harness
        .state_mut()
        .dialogs
        .options
        .as_mut()
        .unwrap()
        .draft
        .connections = 7;
    harness.run_steps(2);
    click(&mut harness, "Restore defaults");
    let d = harness.state().dialogs.options.as_ref().unwrap();
    assert_eq!(d.draft.connections, Settings::default().connections);
    assert_ne!(
        harness.state().applied.connections,
        7,
        "the draft alone is changed"
    );
}

#[test]
fn a_sign_in_pasted_as_an_address_is_kept_by_its_host() {
    let world = world();
    let mut harness = world.harness();
    open_options(&mut harness, dm_desktop::dialogs::Tab::SiteLogins);
    harness
        .state_mut()
        .dialogs
        .options
        .as_mut()
        .unwrap()
        .draft
        .site_logins = vec![dm_shell::settings::SiteLogin {
        host: " https://files.example.com/downloads/ ".into(),
        username: "me".into(),
        password: String::new(),
    }];
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(harness.state().dialogs.options.is_none());
    assert_eq!(
        harness.state().applied.site_logins[0].host,
        "files.example.com"
    );
}

#[test]
fn a_proxy_with_no_port_is_refused_in_a_window_too_short_for_the_tabs() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(760.0, 300.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    open_options(&mut harness, dm_desktop::dialogs::Tab::General);
    {
        let d = harness.state_mut().dialogs.options.as_mut().unwrap();
        d.draft.proxy.enabled = true;
        d.draft.proxy.kind = dm_core::ProxyKind::Http;
        d.draft.proxy.host = "proxy.example".into();
        d.draft.proxy.port = 0;
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    let d = harness.state().dialogs.options.as_ref().expect("closed");
    assert_eq!(d.says, "A proxy needs a port from 1 to 65535.");
    assert_eq!(d.tab, dm_desktop::dialogs::Tab::Advanced);
    harness.run_steps(3);
    assert!(shown(&harness, "A proxy needs a port from 1 to 65535."));
    assert_ne!(
        harness.state().applied.proxy.host,
        "proxy.example",
        "taken anyway"
    );
}

/* ------------------------------------------- questions about rows now gone */

#[test]
fn a_question_about_a_row_no_longer_listed_is_put_away() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().dialogs.name_ask = Some(dm_desktop::dialogs::NameAsk {
        id: 987_654,
        address_name: "get".into(),
        server_name: "report.pdf".into(),
    });
    harness.state_mut().dialogs.page_ask = Some(987_655);
    harness.run_steps(2);
    assert!(harness.state().dialogs.name_ask.is_none());
    assert!(harness.state().dialogs.page_ask.is_none());
}

#[test]
fn download_complete_opens_the_file_with_the_app_picked() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add(&world.url("file.zip"))
        .unwrap();
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let done = harness.state().rows()[0].as_ref().clone();
    let program = world.dir.path().join("Viewer.app");
    *world.fake.chosen.lock().unwrap() = Some(program.clone());
    harness.state_mut().dialogs.complete.insert(done.id);
    harness.run_steps(3);
    click(&mut harness, "Open with…");
    let wanted = format!("{} with {}", done.path, program.display());
    assert!(
        world.fake.opened.lock().unwrap().contains(&wanted),
        "{:?}",
        world.fake.opened.lock().unwrap()
    );
    assert!(!harness.state().dialogs.complete.contains(&done.id));
}
