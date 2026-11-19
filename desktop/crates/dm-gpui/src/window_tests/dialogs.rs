// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The dialogs: what each offers, and that what a person chooses in it is what is done. Find
//! has no Close button to test: Find is the top bar's search box (`docs/decisions.md`
//! 2026-11-10), with no dialog to close. Waiting on the GPUI window: a saved Grabber search
//! under Grabber projects in the side panel (gaps row U40).

use dm_core::State;
use dm_shell::host::Request;

use crate::harness::world::{payload, world, World, PAYLOAD};
use crate::harness::Harness;

mod clipboard;
mod file_info;
mod in_properties;
mod list_files;
mod page_links;
mod parts;

fn finished(h: &mut Harness) -> bool {
    h.rows().iter().any(|r| r.state == State::Completed)
}

fn all_finished(h: &mut Harness) -> bool {
    let rows = h.rows();
    !rows.is_empty() && rows.iter().all(|r| r.state == State::Completed)
}

/// `text` on the clipboard: the system's, which Add links from clipboard reads, and the one
/// the clipboard's watch reads (the fake system's).
fn clip(h: &mut Harness, text: &str) {
    h.cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
    *h.fake.clipboard.lock().unwrap() = Some(text.to_string());
}

/// A download from the page the browser handed over, asked about.
fn handed(world: &World, name: &str) -> Request {
    Request {
        url: world.url(name),
        referrer: world.url("page.html"),
        ask: true,
        ..Default::default()
    }
}

/// A page's links, handed over by the browser: Download links opens for them.
fn hand_links(h: &mut Harness, world: &World, links: Vec<String>) {
    h.hand(Request {
        url: world.url("page.html"),
        referrer: world.url("page.html"),
        links,
        ask: true,
        ..Default::default()
    });
    // Titled for a page's every link or a selection's alike.
    h.until("Download links", |h| h.dialog_open("Download links"));
}

/// Download details for `urls`, as Add URL's OK opens it.
fn details(h: &mut Harness, urls: Vec<String>) {
    h.act(|_, cx| crate::dialogs::details::open(cx.entity().downgrade(), urls, cx));
    h.until("Download details", |h| h.dialog_open("Download details"));
}

/// Download details' Save as holds `name`, the server's, once it has answered.
fn named(h: &mut Harness, name: &str) {
    h.until(&format!("{name} in Save as"), |h| h.shows(name));
}

/// A folder inside the download folder: while a script drives the window a folder typed
/// anywhere else is refused (gaps row 199), and these tests are a script.
fn folder_in(world: &World, name: &str) -> std::path::PathBuf {
    let folder = world.dir.path().join("out").join(name);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

/// A new install's welcome offers the extension for each browser found (each one's page where
/// none is), opens the page asked for, and goes with Later.
#[test]
fn the_welcome_offers_the_extension_for_each_browser() {
    let world = world();
    let mut h = world.harness();
    h.act(|view, cx| view.welcome(cx));
    h.until("the welcome", |h| h.dialog_open("Add dm to your browser"));
    // A browser found is offered as "Add to …"; with none, each store's page by its name,
    // which the browsers' bar behind the dialog shows too: the dialog's is in front.
    let offered = h
        .texts()
        .into_iter()
        .find(|t| t.starts_with("Add to "))
        .unwrap_or_else(|| "Microsoft Edge".to_string());
    h.click(&offered);
    h.until("an extension's page", |h| {
        h.fake.opened.lock().unwrap().iter().any(|page| {
            [
                dm_desktop_base::project::EXTENSION_EDGE,
                dm_desktop_base::project::EXTENSION_FIREFOX,
                dm_desktop_base::project::EXTENSION_CHROME,
            ]
            .contains(&page.as_str())
        })
    });
    h.click("Later");
    h.until("the welcome to go", |h| {
        !h.dialog_open("Add dm to your browser")
    });
}

/// A downloaded file is run by a person's click: a moment after a script pressed a key in the
/// window, Download complete's Open and Open with… and Properties' Open run nothing and say
/// why, as File › Open does; once that moment has passed, a click runs it
/// (`docs/security-report.md`, gaps row 159 f). A script's click on Open is refused outright
/// (`probe::is_run_a_file`), so the clicks here are a person's pointer.
#[test]
fn a_dialogs_open_runs_nothing_a_moment_after_a_script_pressed_a_key() {
    let world = world();
    let mut settings = world.settings();
    settings.general.double_click = "properties".into();
    // The window tests' settings have Download complete off.
    settings.general.show_complete = true;
    let mut h = world.harness_with(settings);
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", finished);
    let row = h.rows()[0].clone();
    h.until("Download complete", |h| h.dialog_open("Download complete"));
    let refused = "Opening a file is done by a person, not by a script";

    // Download complete: Open, then Open with…, run nothing, and the file stays listed.
    h.key("f13", &[]);
    h.person_click("Open", 1);
    h.key("f13", &[]);
    h.person_click("Open with…", 1);
    assert!(
        h.fake.opened.lock().unwrap().is_empty(),
        "{:?}",
        h.fake.opened.lock().unwrap()
    );
    assert!(h.said().contains("done by a person"), "{:?}", h.said());
    assert!(h.dialog_open("Download complete"), "left for the person");
    h.close_dialogs();

    // Properties: its Open the same, said in the window.
    h.person_click("file.zip", 2);
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.key("f13", &[]);
    h.person_click("Open", 1);
    h.until("the refusal said", |h| h.shows(refused));
    assert!(h.fake.opened.lock().unwrap().is_empty());

    // Once the script's moment has passed, a person's click runs it.
    h.until_a_persons_moment();
    h.person_click("Open", 1);
    h.until("the file to be opened", |h| {
        !h.fake.opened.lock().unwrap().is_empty()
    });
    assert_eq!(
        *h.fake.opened.lock().unwrap(),
        std::slice::from_ref(&row.path)
    );
}

#[test]
fn help_opens_the_documentation() {
    let world = world();
    let mut h = world.harness();
    h.command("help.help");
    let opened = h.fake.opened.lock().unwrap().clone();
    assert_eq!(opened, [dm_desktop_base::project::HELP]);
}

/// What is wrong with the range goes once the range is changed: it stood over the count of
/// what was typed since. The GPUI window says it as the range is typed, not only on Start.
#[test]
fn the_series_error_from_start_goes_when_the_range_is_fixed() {
    let world = world();
    let mut h = world.harness();
    h.command("program.add_series");
    h.until("Add numbered series", |h| {
        h.dialog_open("Add numbered series")
    });
    h.type_into("Address with *", &world.url("part*.bin"));
    h.type_into("From", "5");
    h.type_into("To", "1");
    let backwards = "The range runs backwards.";
    h.until_shown(backwards);
    h.click("Start now");
    assert!(
        h.shows(backwards),
        "Start said nothing of a backwards range"
    );
    h.type_into("To", "9");
    h.until("the count of addresses", |h| h.shows_part("5 addresses"));
    assert!(!h.shows(backwards), "the old error still stands");
    assert!(h.rows().is_empty());
}

/// Move/Rename moves several finished files to another folder at once, and their rows follow
/// them.
#[test]
fn move_rename_moves_several_finished_files_together() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("other.pdf"), "Start now");
    h.until("both to complete", |h| {
        h.rows().len() == 2 && all_finished(h)
    });
    h.command("file.select_all");
    h.command("file.move_rename");
    h.until_shown("Moves 2 finished files.");
    let to = folder_in(&world, "moved");
    h.type_into("Folder", &to.display().to_string());
    h.click("OK");
    h.until("the files moved", |h| {
        h.rows()
            .iter()
            .all(|r| std::path::Path::new(&r.path).parent() == Some(to.as_path()))
    });
    for row in h.rows() {
        assert!(
            std::path::Path::new(&row.path).is_file(),
            "{} is not there",
            row.path
        );
    }
}

#[test]
fn two_cancelled_hand_offs_from_a_site_offer_to_leave_it_to_the_browser() {
    let world = world();
    let mut h = world.harness();
    h.settings_to(|s| s.general.ask_panel_downloads = true);
    let hand = |h: &mut Harness, name: &str| {
        h.hand(handed(&world, name));
        h.until("Download details", |h| h.dialog_open("Download details"));
    };
    let question = "Leave this site to the browser?";
    // One cancelled: nothing asked. A typed-in one cancelled: nothing either.
    hand(&mut h, "file.zip");
    h.click("Cancel");
    h.until("Download details to close", |h| h.dialogs().is_empty());
    assert!(!h.shows(question));
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(&world.url("other.pdf"));
    h.click("OK");
    h.until("Download details", |h| h.dialog_open("Download details"));
    h.click("Cancel");
    h.until("Download details to close", |h| h.dialogs().is_empty());
    assert!(!h.shows(question));
    // The second handed one: asked, and "keep" leaves the rules alone.
    hand(&mut h, "other.pdf");
    h.click("Cancel");
    h.until_shown(question);
    h.click("Keep handing them over");
    h.until("the question to go", |h| !h.shows(question));
    assert!(h.view(|v| v.backend.settings().browser.catch.skip_hosts.is_empty()));
    // Twice more, and this time left to the browser: the site joins the list the extension
    // reads.
    hand(&mut h, "file.zip");
    h.click("Cancel");
    h.until("Download details to close", |h| h.dialogs().is_empty());
    hand(&mut h, "file.zip");
    h.click("Cancel");
    h.until_shown(question);
    h.click("Leave them to the browser");
    h.until("the site left to the browser", |h| {
        h.view(|v| v.backend.settings().browser.catch.skip_hosts) == ["127.0.0.1"]
    });
}

/// A dialog asked for again while it is open is brought forward as it stands: what was typed
/// in it is kept, and no second one opens.
#[test]
fn a_dialog_asked_for_while_open_keeps_what_was_typed_in_it() {
    let world = world();
    let mut h = world.harness();
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text("https://e.com/typed.zip");
    h.command("program.add_url");
    h.until("Add URL brought forward", |h| h.dialogs() == ["Add URL"]);
    assert!(
        h.shows("https://e.com/typed.zip"),
        "Add URL asked for again threw away the address"
    );
    h.close_dialogs();
    h.command("downloads.options");
    h.until("Options", |h| h.dialog_open("Options"));
    h.command("downloads.options");
    h.until("one Options", |h| h.dialogs() == ["Options"]);
    h.close_dialogs();
    for (command, open) in [
        ("downloads.scheduler", "Scheduler"),
        ("program.add_series", "Add numbered series"),
        ("program.site_files", "Grabber"),
        ("downloads.speed_limiter.settings", "Speed limiter"),
    ] {
        h.command(command);
        h.until(open, |h| h.dialog_open(open));
        h.command(command);
        h.until(&format!("one {open}"), |h| {
            h.dialogs().iter().filter(|d| *d == open).count() == 1
        });
        h.close_dialogs();
    }
}

/// A download that starts without the window starting it, a browser's hand-off, opens its
/// progress window too, when that is on.
#[test]
fn a_browsers_hand_off_opens_its_progress_window() {
    let world = world();
    let mut h = world.harness_with(world.settings_with(|s| s.general.show_progress = true));
    // As the native host's hand-off lands: in the queue, not through the window's own Add.
    h.view(|v| {
        v.backend
            .add_with(&world.url("stalled.bin"), dm_core::AddOptions::default())
    })
    .unwrap();
    h.until("its progress window", |h| {
        h.dialog_names().iter().any(|d| d == "Download progress")
    });
    // One added for later has not started, and opens nothing.
    let later = h
        .view(|v| {
            v.backend.add_with(
                &world.url("file.zip"),
                dm_core::AddOptions {
                    later: true,
                    queue: dm_core::MAIN_QUEUE.into(),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    h.until("the row", |h| h.rows().iter().any(|r| r.id == later));
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    let progress = h
        .dialog_names()
        .into_iter()
        .filter(|d| d == "Download progress")
        .count();
    assert_eq!(progress, 1, "the row added for later opened one too");
}

/// A fresh link can be got from the browser: its page opens there, and the next download
/// caught becomes the row's link, with no row of its own.
#[test]
fn a_fresh_link_is_taken_from_the_browser() {
    let world = world();
    let mut settings = world.settings();
    settings.advanced.attempts = 3;
    settings.advanced.retry_wait = 0;
    settings.downloads.record_minutes = 0;
    let mut h = world.harness_with(settings);
    h.view(|v| {
        v.backend.add_with(
            &world.url("expires.bin"),
            dm_core::AddOptions {
                referrer: world.url("page.html"),
                ..Default::default()
            },
        )
    })
    .unwrap();
    h.until("the expired link to fail", |h| {
        h.rows().iter().any(|r| matches!(r.state, State::Failed(_)))
    });
    h.until_shown("Get it from the browser");
    h.click("Get it from the browser");
    h.until("the page opened", |h| {
        h.fake
            .opened
            .lock()
            .unwrap()
            .contains(&world.url("page.html"))
    });
    assert!(h.view(|v| v.session.hands.awaiting_link.is_some()));
    // The person clicks the download on the page; the browser hands it over.
    h.hand(Request {
        url: world.url("renewed.bin"),
        referrer: world.url("page.html"),
        caught: true,
        ..Default::default()
    });
    h.until("the link taken", |h| {
        h.view(|v| v.session.hands.awaiting_link.is_none())
    });
    h.until("the download to finish", finished);
    assert_eq!(h.rows().len(), 1, "a row of its own was added");
    assert_eq!(h.rows()[0].url, world.url("renewed.bin"));
}

/// Behind Options the toolbar and the places take no clicks (B42: Options holds the dm window).
/// The progress window is a window of its own beside dm's (B42), which Options does not hold.
#[test]
fn the_window_behind_a_dialog_takes_no_clicks() {
    let world = world();
    let mut h = world.harness();
    h.view(|v| v.backend.add(&world.url("stalled.bin")))
        .unwrap();
    h.until("it to run", |h| {
        h.rows().iter().any(|r| r.state == State::Running)
    });
    h.command("downloads.options");
    h.until("Options", |h| h.dialog_open("Options"));
    // The toolbar's Add URL, not a word of Options: the main window's own.
    let main_add_url = h.texts().iter().filter(|t| *t == "Add URL").count() - 1;
    h.call(
        "ui.click",
        serde_json::json!({ "text": "Add URL", "nth": main_add_url }),
    )
    .unwrap();
    assert!(!h.dialog_open("Add URL"), "Add URL opened behind Options");
    let before = h.view(|v| v.list.place.clone());
    h.click("Finished");
    assert_eq!(
        h.view(|v| v.list.place.clone()),
        before,
        "a place was picked behind Options"
    );
    // Nor the toolbar's Stop.
    h.select_by_port("stalled.bin");
    h.click("Stop");
    for _ in 0..10 {
        h.until("a wake", |_| true);
    }
    assert_eq!(
        h.rows()[0].state,
        State::Running,
        "Stop worked behind Options"
    );
}

/// A new queue's name typed in Move to queue is the name, not "Main queueNight" after the
/// picked queue's name standing in the field.
#[test]
fn move_to_queue_makes_the_queue_named() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("other.pdf"), "Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    h.select_by_port("other.pdf");
    h.command("file.move_to_queue");
    h.until("Move to queue", |h| h.dialog_open("Move to queue"));
    h.type_into("Or a new queue", "Night");
    h.key("enter", &[]);
    h.until("the row in Night", |h| h.rows()[0].queue == "Night");
}

/// Letters typed into a number box leave its number as it was, and the box says so once it is
/// left, rather than appearing to have taken them ("abc" in a size floor silently stayed 0).
#[test]
fn a_number_box_says_when_what_was_typed_is_not_a_number() {
    let world = world();
    let mut h = world.harness();
    h.click("Options");
    h.until("Options", |h| h.dialog_open("Options"));
    h.click("Advanced");
    let label = "Time out a silent connection after (seconds)";
    h.until_shown(label);
    let before = h.view(|v| v.backend.settings().advanced.read_timeout);
    let said = "Numbers only: the one before was kept";
    h.type_into(label, "abc");
    assert!(!h.shows(said), "said while still typing");
    // Left for another box (Attempts', which holds 15).
    h.person_click("15", 1);
    h.until("the box to say so", |h| h.shows(said));
    h.click("OK");
    h.until("Options to close", |h| !h.dialog_open("Options"));
    assert_eq!(
        h.view(|v| v.backend.settings().advanced.read_timeout),
        before,
        "the number changed"
    );
}

/// Someone who installed only the app is shown how to add dm to their browser, one click from
/// its store page, until a browser connects or they say not now.
#[test]
fn a_new_user_is_shown_how_to_add_dm_to_their_browser() {
    let home = crate::harness::home();
    let seen = home.join("browser.seen");
    let hidden = home.join("browser-hint.hidden");
    let _ = std::fs::remove_file(&seen);
    let _ = std::fs::remove_file(&hidden);
    let world = world();
    let mut h = world.harness();
    h.until_shown("Microsoft Edge");
    h.click("Microsoft Edge");
    h.until("the store page", |h| {
        h.fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o == dm_desktop_base::project::EXTENSION_EDGE)
    });
    h.click("Not now");
    h.until("the bar to go", |h| !h.shows("Microsoft Edge"));
    assert!(hidden.exists(), "not now is remembered");

    // A browser that has connected: nothing offered, from the start.
    std::fs::remove_file(&hidden).unwrap();
    std::fs::write(&seen, b"").unwrap();
    let mut h = world.harness();
    for _ in 0..10 {
        h.until("a wake", |_| true);
    }
    let offered = h.shows("Microsoft Edge");
    std::fs::remove_file(&seen).unwrap();
    assert!(!offered, "offered to someone whose browser has connected");
}

/// With a dialog open, a person's click on the menu bar opens nothing over it, so dialogs are
/// not stacked by hand. Without the dialog, the same clicks open About.
#[test]
fn the_menus_do_not_open_dialogs_over_a_dialog() {
    let world = world();
    let mut h = world.harness();
    h.command("downloads.options");
    h.until("Options", |h| h.dialog_open("Options"));
    h.click("Help");
    h.click("About dm");
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    assert!(!h.dialog_open("About dm"), "About opened over Options");
    h.close_dialogs();
    h.click("Help");
    h.click("About dm");
    h.until("About dm", |h| h.dialog_open("About dm"));
}

/// The new install's double-click opens a finished file, as a file manager's does; one that is
/// still going shows its progress window.
#[test]
fn the_default_double_click_opens_a_finished_file_and_shows_progress_for_one_still_going() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("one to finish", finished);
    let path = h.row("file.zip").path;
    h.until_a_persons_moment();
    h.person_click("file.zip", 2);
    h.until("the file opened", |h| {
        !h.fake.opened.lock().unwrap().is_empty()
    });
    assert_eq!(*h.fake.opened.lock().unwrap(), [path]);
    h.person_click("stalled.bin", 2);
    h.until("the progress window", |h| {
        h.dialog_names().iter().any(|d| d == "Download progress")
    });
    assert!(
        !h.dialog_open("File properties"),
        "Properties opened instead of the progress window"
    );
}

/// The close button asks once whether dm keeps running in the tray, and what is chosen is
/// kept: Keep running goes to the tray, Exit quits, and neither is asked again. The window's
/// tests make no tray, so the close button would end dm at once (`close_asked`): the question
/// is asked as the close button asks it where there is one.
#[test]
fn the_close_question_is_asked_once_and_the_answer_is_kept() {
    let world = world();
    let mut h = world.harness();
    assert_eq!(h.view(|v| v.backend.settings().general.on_close), "ask");
    let question = "Keep running in the tray?";
    h.act(|view, cx| view.ask_close(cx));
    h.until_shown(question);
    h.click("Keep running");
    h.until("the window sent to the tray", |h| {
        h.view(|v| v.system.hidden)
    });
    assert!(!h.shows(question));
    assert_eq!(h.view(|v| v.backend.settings().general.on_close), "tray");

    // The other answer, from a fresh question.
    h.act(|view, cx| view.show_main(cx));
    h.settings_to(|s| s.general.on_close = "ask".into());
    h.act(|view, cx| view.ask_close(cx));
    h.until_shown(question);
    h.click("Exit");
    h.until("Exit to quit", |_| crate::dialogs::question::is_quitting());
    assert_eq!(h.view(|v| v.backend.settings().general.on_close), "exit");
}

/// The welcome is three steps: the browser, where downloads go, and Done. The folder shown is
/// the one in force, and a change is kept on Done. A folder a script typed is kept only from a
/// person's Done: settings that say where files go are a person's to change (gaps row 199).
#[test]
fn the_welcome_confirms_where_downloads_go_and_ends_with_done() {
    let world = world();
    let mut h = world.harness();
    let was = h.view(|v| v.backend.settings().folders.download_dir);
    h.act(|view, cx| view.welcome(cx));
    h.until("the welcome", |h| h.dialog_open("Add dm to your browser"));
    h.until_shown("Download folder");
    let shown = was.display().to_string();
    assert!(h.shows_in_dialog(&shown), "not the folder in force");
    let elsewhere = world.dir.path().join("elsewhere");
    h.person_click(&shown, 1);
    h.key("a", &["cmd"]);
    h.type_text(&elsewhere.display().to_string());
    h.until_a_persons_moment();
    h.person_click("Done", 1);
    h.until("the welcome to close", |h| {
        !h.dialog_open("Add dm to your browser")
    });
    let now = h.view(|v| v.backend.settings().folders.download_dir);
    assert_eq!(now, elsewhere);
}

/// `--new-download` (the taskbar jump list's New download), left in the inbox, opens Add URL;
/// with nothing left, nothing opens.
#[test]
fn a_new_download_order_opens_add_url_and_brings_the_window_forward() {
    let _inbox = crate::harness::inbox();
    let world = world();
    let mut h = world.harness();
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert!(!h.dialog_open("Add URL"), "opened with no order");
    let inbox = dm_shell::host::inbox(&dm_shell::settings::home());
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::write(inbox.join(dm_shell::shell::NEW_DOWNLOAD_FILE), b"").unwrap();
    h.until("Add URL", |h| h.dialog_open("Add URL"));
    assert!(
        !inbox.join(dm_shell::shell::NEW_DOWNLOAD_FILE).exists(),
        "taken, so not again"
    );
}

/// Help has reference A's items (gaps row 91); Tip of the day shows a tip and Next tip the
/// next; Tell a friend is an email for the person's own mail program, nothing sent by dm.
#[test]
fn help_has_reference_as_items_and_tip_of_the_day_shows_the_next_tip() {
    let world = world();
    let mut h = world.harness();
    h.click("Help");
    for item in [
        "Tutorials",
        "Queues and the Scheduler",
        "Grabber help",
        "Tip of the day",
        "Home page",
        "Contact support",
        "Tell a friend",
    ] {
        assert!(h.shows(item), "no {item} in Help: {:?}", h.texts());
    }
    h.click("Tip of the day");
    let tips = dm_desktop_base::project::tips();
    h.until("the first tip", |h| h.shows(tips[0]));
    h.click("Next tip");
    h.until("the next tip", |h| h.shows(tips[1]));
    h.click("Close");
    h.until("the tip to close", |h| h.dialogs().is_empty());
    let mail = dm_desktop_base::project::tell_a_friend();
    assert!(mail.starts_with("mailto:?subject="), "{mail}");
    assert!(mail.contains("github.com"), "{mail}");
}
