// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the window does around its list between a person's clicks: drops, the problems it
//! says, a file due to go that cannot, a schedule's date that is over, a sort by status, the
//! limits said in the status line. Ported from the egui window's `tests/app_cover.rs` and
//! `tests/app_more_cover.rs`.

use dm_core::State;

use crate::harness::world::world;
use crate::harness::{settings_file, Harness};

/// The problems said above the list.
fn problems(h: &mut Harness) -> Vec<String> {
    h.view(|view| view.problems.clone())
}

/// `paths` dropped on the list, as the system hands a drop to the window.
fn drop_on_list(h: &mut Harness, paths: Vec<std::path::PathBuf>) {
    let at = gpui::point(gpui::px(400.0), gpui::px(300.0));
    let window = h.window;
    h.cx.update(|cx| {
        let _ = window.update(cx, |_, window, cx| {
            let paths = gpui::ExternalPaths(paths.into());
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
}

/// A text file of links dropped on the list: its links are downloaded.
#[test]
fn a_dropped_file_of_links_is_downloaded() {
    let world = world();
    let mut h = world.harness();
    let links = world.dir.path().join("links.txt");
    std::fs::write(&links, format!("{}\n", world.url("file.zip"))).unwrap();
    drop_on_list(&mut h, vec![links]);
    h.until("the dropped link to finish", |h| {
        h.rows()
            .iter()
            .any(|r| r.url.ends_with("/file.zip") && r.state == State::Completed)
    });
    assert_eq!(h.rows().len(), 1);
}

/// The command run on each finished file, where it cannot start, is a problem said; the
/// sounds a person chose for a finish and for the queue running dry are played (here, as the
/// fake system plays any sound: its chime).
#[test]
fn a_finished_files_command_that_cannot_start_is_a_problem_and_chosen_sounds_play() {
    let world = world();
    let mut settings = world.settings();
    settings.on_complete = world
        .dir
        .path()
        .join("no-such-program")
        .display()
        .to_string();
    settings.sound_on_complete = true;
    for event in ["complete", "queue_done"] {
        settings
            .sounds
            .insert(event.into(), format!(" /sounds/{event}.wav "));
    }
    let mut h = world.harness_with(settings);
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": [world.url("file.zip")] }),
    )
    .unwrap();
    h.until("the program's failure to be said", |h| {
        problems(h)
            .iter()
            .any(|p| p.contains("no-such-program") && p.contains("could not start"))
    });
    let chimes = || {
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .filter(|o| *o == "chime")
            .count()
    };
    h.until("the finish and the dry queue's sounds", |_| chimes() >= 2);
}

/// "Delete file after" with a file that cannot be moved to the Trash: it stays with its row,
/// is said, and is not tried again. Made so by a read-only folder, which only Unix's
/// permissions give.
#[cfg(unix)]
#[test]
fn a_file_due_for_deletion_that_cannot_be_moved_stays_and_is_said() {
    use std::os::unix::fs::PermissionsExt;
    let world = world();
    let mut h = world.harness();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": [world.url("file.zip")] }),
    )
    .unwrap();
    h.until("the download to finish", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = h.rows()[0].clone();
    let path = std::path::PathBuf::from(&row.path);
    let folder = path.parent().unwrap().to_path_buf();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    // As root the folder is no bar: nothing to see then.
    if std::fs::write(folder.join("probe"), b"x").is_ok() {
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("the folder can be written read-only (root?); skipped");
        return;
    }
    h.view(|view| view.backend.set_delete_after(row.id, Some(1)));
    h.until("the failure to be said", |h| {
        h.said().starts_with("Could not delete")
    });
    let kept = h.view(|view| view.backend.row(row.id));
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(path.exists(), "the file was deleted");
    let kept = kept.expect("the row was removed");
    assert_eq!(kept.delete_after, None, "tried again");
}

/// A cookies file moved away since it was chosen does not stop a change of settings: it is a
/// problem said beside it.
#[test]
fn a_change_of_settings_with_a_cookies_file_gone_is_made_and_the_problem_said() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    let cookies = world.dir.path().join("cookies.txt");
    std::fs::write(&cookies, "# Netscape HTTP Cookie File\n").unwrap();
    let named = cookies.display().to_string();
    h.settings_to(|s| s.cookies_file = named);
    std::fs::remove_file(&cookies).unwrap();
    h.settings_to(|s| s.hinted_big = !s.hinted_big);
    h.until("the cookies file's problem", |h| {
        problems(h)
            .iter()
            .any(|p| p.contains("downloads go without its cookies"))
    });
    h.settings_to(|s| s.cookies_file = String::new());
}

/// A once-only date that is over is put away, for every queue and for a queue of its own,
/// and the weekly hours are back.
#[test]
fn a_once_only_date_that_is_over_is_put_away() {
    let _file = settings_file();
    let world = world();
    let spent = dm_core::Schedule {
        once_at: Some("2020-01-01 01:00".into()),
        enabled: true,
        from: "01:00".into(),
        to: "02:00".into(),
        ..dm_core::Schedule::default()
    };
    let mut settings = world.settings();
    settings.schedule = spent.clone();
    settings.queue_schedules.insert("Nightly".into(), spent);
    let mut h = world.harness_with(settings);
    h.until("the dates to be put away", |h| {
        let kept = h.view(|view| view.backend.settings());
        kept.schedule.once_at.is_none()
            && kept
                .queue_schedules
                .get("Nightly")
                .is_some_and(|s| s.once_at.is_none())
    });
}

/// Sorted by Status: running, queued, stopped, cancelled.
#[test]
fn sorted_by_status_the_rows_go_in_the_order_of_their_state() {
    let world = world();
    let mut settings = world.settings();
    settings.concurrency = 1;
    let mut h = world.harness_with(settings);
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": [world.url("stalled.bin"), world.url("half.mp4")] }),
    )
    .unwrap();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": [world.url("cancelled.bin"), world.url("stopped.bin")],
                            "later": true }),
    )
    .unwrap();
    let cancelled = h
        .view(|view| view.backend.list())
        .iter()
        .find(|r| r.url.ends_with("cancelled.bin"))
        .unwrap()
        .id;
    h.view(|view| view.backend.cancel(cancelled));
    h.until("one running, one queued, one cancelled", |h| {
        let states: Vec<State> = h.rows().iter().map(|r| r.state.clone()).collect();
        states.contains(&State::Running)
            && states.contains(&State::Queued)
            && states.contains(&State::Cancelled)
    });
    h.click("Status");
    h.until("the sort by status", |h| {
        h.view(|view| {
            view.sort
                .is_some_and(|(c, _)| c == dm_desktop_base::columns::Column::Status)
        })
    });
    let order: Vec<&'static str> = h.view(|view| {
        if view.sort.is_some_and(|(_, ascending)| !ascending) {
            view.shown.reverse();
        }
        view.shown
            .iter()
            .map(|&i| dm_desktop_base::rows::kind(&view.rows[i].state))
            .collect()
    });
    assert_eq!(order, ["running", "queued", "paused", "cancelled"]);
    h.command("downloads.stop_all");
}

/// The download limit reached is said in the status line, with how long downloads are held.
#[test]
fn the_download_limit_reached_is_said_in_the_status_line() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.settings_to(|s| s.limit_mb = 1);
    let urls: Vec<String> = (0..6).map(|_| world.url("file.zip")).collect();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": urls, "again": true }),
    )
    .unwrap();
    h.until("the limit reached to be said", |h| {
        h.shows_part("download limit of 1 MB reached, held for ")
    });
    h.settings_to(|s| s.limit_mb = 0);
}

/// Several links added at once where some cannot be: the rest go in, and how many did not is
/// said.
#[test]
fn links_that_cannot_be_added_are_counted_and_the_rest_go_in() {
    let world = world();
    let mut h = world.harness();
    let later = dm_core::AddOptions {
        later: true,
        ..Default::default()
    };
    let one_bad = vec![world.url("a.bin"), "http://".to_string()];
    let added = h.update(|view, cx| view.add(one_bad, later.clone(), cx));
    assert_eq!(added, Ok(1));
    assert!(
        h.said().starts_with("One of the links was not added: "),
        "{}",
        h.said()
    );
    let two_bad = vec![
        "http://".to_string(),
        world.url("b.bin"),
        "https://".to_string(),
    ];
    let added = h.update(|view, cx| view.add(two_bad, later.clone(), cx));
    assert_eq!(added, Ok(1));
    assert!(
        h.said().starts_with("2 of the links were not added: "),
        "{}",
        h.said()
    );
    let refused = h.update(|view, cx| view.add(vec!["http://".to_string()], later, cx));
    assert!(refused.unwrap_err().contains("names no site"));
    assert_eq!(h.view(|view| view.backend.list().len()), 2);
}

/// `events` sent to the window `to`, as the system hands them to it.
fn send_to(h: &mut Harness, to: gpui::AnyWindowHandle, events: Vec<gpui::PlatformInput>) {
    h.cx.update(|cx| {
        let _ = to.update(cx, |_, window, cx| {
            for event in events {
                window.dispatch_event(event, cx);
            }
        });
    });
}

/// The floating drop target, a window of its own: what is dropped on it is taken. (Its
/// right-click menu is gpui-component's, in the target's own window, which the words on screen
/// do not reach; its two items are the window's own actions.)
#[test]
fn the_floating_drop_target_takes_what_is_dropped_on_it() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.click("View");
    h.click("Drop target");
    h.until("the drop target", |h| {
        h.view(|view| view.drop_window)
            .is_some_and(|w| h.cx.update(|cx| cx.windows().contains(&w)))
    });
    let target = h.view(|view| view.drop_window).unwrap();
    let at = gpui::point(gpui::px(20.0), gpui::px(20.0));
    let links = world.dir.path().join("more.txt");
    std::fs::write(&links, format!("{}\n", world.url("other.pdf"))).unwrap();
    use gpui::{FileDropEvent, PlatformInput};
    send_to(
        &mut h,
        target,
        vec![
            PlatformInput::FileDrop(FileDropEvent::Entered {
                position: at,
                paths: gpui::ExternalPaths(vec![links].into()),
            }),
            PlatformInput::FileDrop(FileDropEvent::Submit { position: at }),
        ],
    );
    h.until("the dropped link to be added", |h| {
        h.rows().iter().any(|r| r.url.ends_with("/other.pdf"))
    });
}
