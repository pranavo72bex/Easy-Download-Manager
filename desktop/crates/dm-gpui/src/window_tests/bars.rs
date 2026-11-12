// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The bars above the list, the empty list's first step, and commands on rows a menu or a
//! key runs: what each opens and says. Ported from the egui window's
//! `tests/app_more_cover.rs`.

use dm_core::State;
use dm_shell::app::commands::Command;

use crate::harness::world::world;
use crate::harness::{settings_file, Harness};

/// `urls` added through the port, started or for later.
fn add(h: &mut Harness, urls: &[String], later: bool) {
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": urls, "later": later }),
    )
    .unwrap();
    h.until("the rows to be read", |h| h.rows().len() >= urls.len());
}

/// The browser bar's buttons each open their own store's page.
#[test]
fn the_browser_bar_opens_each_browsers_page() {
    let world = world();
    let mut h = world.harness();
    h.until_shown("Add dm to your browser:");
    for browser in ["Firefox", "Chrome and others", "Microsoft Edge"] {
        let before = world.fake.opened.lock().unwrap().len();
        h.click(browser);
        h.until(&format!("{browser}'s page"), |_| {
            world.fake.opened.lock().unwrap().len() > before
        });
        let opened = world.fake.opened.lock().unwrap().clone();
        assert!(
            opened.last().unwrap().starts_with("https://"),
            "{browser}: {opened:?}"
        );
    }
}

/// A problem said above the list stays until it is dismissed.
#[test]
fn a_problem_above_the_list_is_dismissed() {
    let world = world();
    let mut h = world.harness();
    h.update(|view, cx| {
        view.problems.push("the disk said no to something".into());
        cx.notify();
    });
    h.until("the problem to be shown", |h| {
        h.shows_part("disk said no to something")
    });
    h.click("Dismiss");
    h.until("the problem to go", |h| h.view(|view| view.problems.is_empty()));
}

/// An empty list offers its first step as a button: Add URL.
#[test]
fn an_empty_list_offers_to_add_a_download() {
    let world = world();
    let mut h = world.harness();
    h.until_shown("Add a download");
    h.click("Add a download");
    h.until("Add URL", |h| h.dialog_open("Add URL"));
}

/// On a row added for later, Update download link and Show progress open their windows; on a
/// finished row Show progress opens none; the clipboard with no link in it is said.
#[test]
fn commands_on_a_row_open_their_windows_and_an_empty_clipboard_is_said() {
    let world = world();
    let mut h = world.harness();
    add(&mut h, &[world.url("later.zip")], true);
    h.select("later.zip");
    h.run(Command::UpdateLink);
    h.until("Update download link", |h| !h.dialogs().is_empty());
    h.close_dialogs();
    h.run(Command::ShowProgress);
    h.until("its progress window", |h| {
        h.dialog_names().iter().any(|d| d == "Download progress")
    });
    h.close_dialogs();

    h.cx
        .write_to_clipboard(gpui::ClipboardItem::new_string("only words, no address".into()));
    h.run(Command::AddFromClipboard);
    assert_eq!(h.said(), "There are no links on the clipboard.");
    assert!(h.dialogs().is_empty());

    add(&mut h, &[world.url("file.zip")], false);
    h.until("file.zip to finish", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
    h.select("file.zip");
    h.run(Command::ShowProgress);
    assert!(
        !h.dialog_names().iter().any(|d| d == "Download progress"),
        "a finished row's progress window: {:?}",
        h.dialog_names()
    );
}

/// Copy details of a failed row puts its block on the clipboard and says how many; When done
/// is set by its command.
#[test]
fn copy_details_of_a_failed_row_and_when_done() {
    let world = world();
    let mut h = world.harness();
    add(&mut h, &[world.url("secret.zip")], false);
    h.until("the download to fail", |h| {
        h.rows().iter().any(|r| matches!(r.state, State::Failed(_)))
    });
    h.select("secret.zip");
    h.run(Command::CopyDetails);
    let copied = h
        .cx
        .read_from_clipboard()
        .and_then(|c| c.text())
        .expect("nothing was copied");
    assert!(copied.contains("State: failed"), "{copied}");
    assert_eq!(h.said(), "Copied the details of 1 download.");

    use dm_shell::app::power::WhenDone;
    h.run(Command::WhenDoneQuit);
    assert_eq!(h.view(|view| view.session.when_done), WhenDone::Quit);
    h.run(Command::WhenDoneNothing);
    assert_eq!(h.view(|view| view.session.when_done), WhenDone::Nothing);
}

/// Start queue while the schedule is shut says the queue waits for it; Stop queue says how
/// many it stopped.
#[test]
fn start_queue_under_a_shut_schedule_and_stop_queue() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    add(&mut h, &[world.url("held.zip")], true);
    // A schedule on no day there is: shut, and never opening.
    h.settings_to(|s| {
        s.schedule = dm_core::Schedule {
            enabled: true,
            from: "01:00".into(),
            to: "02:00".into(),
            days: vec![9],
            ..dm_core::Schedule::default()
        }
    });
    h.run(Command::StartQueue);
    assert!(
        h.said()
            .ends_with("is waiting for the schedule; it starts by itself when the schedule opens."),
        "{}",
        h.said()
    );
    h.run(Command::StopQueue);
    assert!(
        h.said().starts_with("Stopped 1 download in "),
        "{}",
        h.said()
    );
    h.settings_to(|s| s.schedule = dm_core::Schedule::default());
}

/// Enter on the list opens the selected row's progress window.
#[test]
fn enter_on_the_list_shows_the_selected_rows_progress() {
    let world = world();
    let mut h = world.harness();
    add(&mut h, &[world.url("enter.zip")], true);
    h.select("enter.zip");
    h.key("enter", &[]);
    h.until("its progress window", |h| {
        h.dialog_names().iter().any(|d| d == "Download progress")
    });
}
