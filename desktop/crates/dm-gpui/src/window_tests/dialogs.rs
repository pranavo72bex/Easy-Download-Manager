// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The dialogs: what each offers, and that what a person chooses in it is what is done. Moved
//! from the egui window's tests. Not moved: Find's Close button (the GPUI window's Find is the
//! top bar's search box, `docs/decisions.md` 2026-11-10, with no dialog to close), and two of
//! egui's own drawing (no field outlined in egui's selection stroke; a dialog with egui's
//! deepest shadow standing whole inside the main window, where a GPUI dialog is a window of
//! its own). Waiting on the GPUI window: Browse…, Import and Export with no system chooser
//! taking a typed path (gaps row U41), and a saved Grabber search under Grabber projects in the
//! side panel (gaps row U40).

use dm_core::State;
use dm_shell::host::Request;

use crate::harness::world::{payload, world, World, PAYLOAD};
use crate::harness::Harness;

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
    h.act(|_, cx| crate::details::open(cx.entity().downgrade(), urls, cx));
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

#[test]
fn a_video_from_the_browsers_panel_opens_download_file_info_with_its_quality() {
    let world = world();
    let mut h = world.harness();
    h.settings_to(|s| s.ask_panel_downloads = true);
    let page = world.url("file.zip");
    h.hand(Request {
        url: page.clone(),
        referrer: "https://video.example/watch".into(),
        quality: Some("720".into()),
        label: Some("MP4 1280×720".into()),
        ask: true,
        ..Default::default()
    });
    h.until("Download details", |h| h.dialog_open("Download details"));
    h.until_shown("MP4 1280×720");
    assert!(h.rows().is_empty(), "nothing starts before it is asked");

    h.click("Start now");
    h.until("the row", |h| !h.rows().is_empty());
    let row = h.rows()[0].clone();
    assert_eq!(row.url, page);
    assert_eq!(row.quality.as_deref(), Some("720"));
    assert_eq!(row.referrer, "https://video.example/watch");
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

#[test]
fn a_pages_links_open_the_download_links_dialog() {
    let world = world();
    let mut h = world.harness();
    h.settings_to(|s| s.ask_panel_downloads = true);
    hand_links(
        &mut h,
        &world,
        vec![
            world.url("file.zip"),
            world.url("other.pdf"),
            world.url("next.html"),
        ],
    );
    // Each link's server is asked for its kind and size.
    h.until("the links to be checked", |h| {
        h.shows("Archives")
            && h.texts()
                .iter()
                .filter(|t| **t == dm_desktop_base::format::bytes(256 * 1024))
                .count()
                >= 2
    });
    // "Hide HTML files" is on by default: the page is not offered.
    assert!(!h.shows("next.html"), "{:?}", h.texts());
    h.click("other.pdf");
    h.click("Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let rows = h.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].url, world.url("file.zip"));
    assert_eq!(rows[0].queue, dm_core::MAIN_QUEUE);
    assert_eq!(rows[0].state, State::Paused, "for later: not started");
    assert_eq!(rows[0].referrer, world.url("page.html"));
    // The size the dialog showed is on the row, though nothing was fetched.
    assert_eq!(rows[0].progress.downloaded, 0);
    assert_eq!(
        rows[0].progress.total,
        Some(PAYLOAD as u64),
        "the size the dialog showed was lost"
    );
    h.until("the size the dialog showed, in the list", |h| {
        h.shows(&dm_desktop_base::format::bytes(256 * 1024))
    });

    // Only the types typed are listed, and only those are added.
    hand_links(
        &mut h,
        &world,
        vec![world.url("file.zip"), world.url("other.pdf")],
    );
    h.type_into("Only these types", "PDF");
    // file.zip is in the list behind the dialog, so its line here is known by what follows
    // its name.
    h.until("file.zip filtered out", |h| {
        !h.shows("file.zip (already in the list)") && h.shows("other.pdf")
    });
    h.click("Check all");
    h.click("Add for later");
    h.until("other.pdf added", |h| h.rows().len() == 2);
    assert!(h.rows().iter().any(|r| r.url == world.url("other.pdf")));

    // Start now starts them.
    hand_links(&mut h, &world, vec![world.url("deep.pdf")]);
    h.until_shown("deep.pdf");
    h.click("Start now");
    h.until("the started link to finish", |h| {
        h.rows()
            .iter()
            .any(|r| r.url.ends_with("/deep.pdf") && r.state == State::Completed)
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
    settings.double_click = "properties".into();
    // The window tests' settings have Download complete off.
    settings.show_complete = true;
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
fn a_double_click_opens_properties_and_open_runs_a_finished_file() {
    let world = world();
    let mut settings = world.settings();
    // Options › General's other choice; "opens the file" is the new install's, below.
    settings.double_click = "properties".into();
    let mut h = world.harness_with(settings);
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("one to finish", finished);

    // One still going: its progress window, and not Properties.
    h.click("stalled.bin");
    h.call(
        "ui.click",
        serde_json::json!({ "text": "stalled.bin", "double": true }),
    )
    .unwrap();
    h.until("the progress window", |h| {
        h.dialog_names().iter().any(|d| d == "Download progress")
    });
    assert!(
        !h.dialog_open("File properties"),
        "Properties opened as well"
    );
    // Its Properties are still there from the menu, with nothing yet to open.
    h.command("file.properties");
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.until_a_persons_moment();
    h.person_click("Open", 1);
    assert!(
        h.fake.opened.lock().unwrap().is_empty(),
        "nothing to open yet"
    );
    h.close_dialogs();

    // A finished one: Properties, whose Open runs the file.
    let row = h.row("file.zip");
    h.call(
        "ui.click",
        serde_json::json!({ "text": "file.zip", "double": true }),
    )
    .unwrap();
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.until_a_persons_moment();
    h.person_click("Open", 1);
    h.until("the file to be opened", |h| {
        !h.fake.opened.lock().unwrap().is_empty()
    });
    assert_eq!(
        *h.fake.opened.lock().unwrap(),
        std::slice::from_ref(&row.path)
    );

    // Options › General's "opens the file": a person's double-click opens a finished one
    // straight away; one still going shows its progress.
    h.close_dialogs();
    h.fake.opened.lock().unwrap().clear();
    h.settings_to(|s| s.double_click = "open".into());
    h.until_a_persons_moment();
    h.person_click("file.zip", 2);
    h.until("the file to be opened", |h| {
        !h.fake.opened.lock().unwrap().is_empty()
    });
    assert_eq!(
        *h.fake.opened.lock().unwrap(),
        std::slice::from_ref(&row.path)
    );
    assert!(
        !h.dialog_open("File properties"),
        "Properties opened instead"
    );
    h.person_click("stalled.bin", 2);
    h.until("the progress window", |h| {
        h.dialog_names().iter().any(|d| d == "Download progress")
    });
}

/// Download details offers the folder a kind of file was last put in, and only for that kind;
/// choosing the kind's own folder again forgets it.
#[test]
fn download_details_offers_the_folder_last_used_for_that_kind() {
    let world = world();
    let mut h = world.harness();
    let folder = folder_in(&world, "my zips");
    let shown = folder.display().to_string();
    details(&mut h, vec![world.url("file.zip")]);
    named(&mut h, "file.zip");
    assert!(!h.shows_in_dialog(&shown));
    h.type_into("Save to folder", &shown);
    h.click("Add for later");
    h.until("the folder kept for archives", |h| {
        h.view(|v| v.backend.settings().last_folders.get("Archives").cloned())
            == Some(shown.clone())
    });
    // A document is not an archive.
    details(&mut h, vec![world.url("other.pdf")]);
    named(&mut h, "other.pdf");
    assert!(
        !h.shows_in_dialog(&shown),
        "an archive's folder offered for a PDF"
    );
    h.click("Cancel");
    // The next archive is offered it.
    details(&mut h, vec![world.url("file.zip")]);
    named(&mut h, "file.zip");
    h.until("the folder offered", |h| h.shows_in_dialog(&shown));
    // Its own folder chosen again: forgotten.
    h.type_into("Save to folder", "");
    h.click("Add for later");
    h.until("the folder forgotten", |h| {
        h.view(|v| v.backend.settings().last_folders.is_empty())
    });
}

#[test]
fn the_file_info_dialog_puts_the_file_where_and_as_what_it_says() {
    let world = world();
    let mut h = world.harness();
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(&world.url("file.zip"));
    h.click("OK");
    h.until("Download details", |h| h.dialog_open("Download details"));
    let folder = folder_in(&world, "picked");
    h.type_into("Save to folder", &folder.to_string_lossy());
    // Save as holds the server's name once it answers: replaced, as a person replaces it.
    named(&mut h, "file.zip");
    h.type_into("Save as", "renamed.zip");
    h.type_into("Note", "the one from the page");
    h.click("Start now");
    h.until("the download to complete", finished);
    let row = h.rows()[0].clone();
    assert_eq!(std::path::Path::new(&row.path), folder.join("renamed.zip"));
    assert_eq!(row.description, "the one from the page");
    h.until("the Note column", |h| h.shows("the one from the page"));
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
fn properties_keep_the_login_out_of_the_address_box() {
    let password = crate::test_secrets::secret("password");
    let world = world();
    let mut h = world.harness();
    let url = world
        .url("private.bin")
        .replace("http://", &format!("http://me:{password}@"));
    h.view(|v| {
        v.backend.add_with(
            &url,
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
    })
    .unwrap();
    h.until("the row", |h| !h.rows().is_empty());
    let name = dm_shell::app::names::name_of(&h.rows()[0]);
    h.select_by_port(&name);
    h.command("file.properties");
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.until_shown("me");
    assert!(
        !h.texts().iter().any(|t| t.contains(&password)),
        "the password is on screen: {:?}",
        h.texts()
    );
    // Changing the password changes it on the download.
    h.type_into("Password", "better");
    h.click("OK");
    h.until("the new password", |h| {
        h.rows()[0].url.contains("me:better@")
    });
}

#[test]
fn properties_change_the_description_and_refresh_an_expired_address() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("gone.bin"), "Start now");
    // A 404 is told in a window of its own (B41), answered before the list is used.
    h.until_shown("The file doesn't exist");
    h.click("OK");
    h.until("the missing file to fail", |h| {
        h.rows().iter().any(|r| matches!(r.state, State::Failed(_)))
    });
    h.select_by_port("gone.bin");
    h.command("file.properties");
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.type_into("Note", "fixed link");
    h.type_into("Address", &world.url("file.zip"));
    h.click("OK");
    h.until("the row changed", |h| {
        h.rows()[0].description == "fixed link"
    });
    let row = h.rows()[0].clone();
    assert!(row.url.ends_with("/file.zip"), "{}", row.url);
    // Not yet named by a server, the row goes by its new address's name.
    h.select_by_port("file.zip");
    // A failed row is retried, and its button says so.
    h.until_shown("Retry");
    assert!(!h.shows("Resume"), "{:?}", h.texts());
    h.click("Retry");
    h.until("the refreshed download to complete", all_finished);
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

#[test]
fn the_site_file_finder_finds_the_files_a_page_links_to() {
    let world = world();
    let mut h = world.harness();
    h.click("Grabber");
    h.until("the Grabber", |h| h.dialog_open("Grabber"));
    h.type_into("Start page", &world.url("page.html"));
    h.type_into("File types", "zip pdf");
    h.click("Find files");
    h.until("the files found", |h| h.shows_part("Files found: 2 "));
    h.click("Add for later");
    h.until("the rows", |h| h.rows().len() == 2);
    let urls: Vec<String> = h.rows().iter().map(|r| r.url.clone()).collect();
    assert!(urls.iter().any(|u| u.ends_with("/file.zip")));
    assert!(urls.iter().any(|u| u.ends_with("/other.pdf")));
}

/// The Grabber's Start now adds what it found and starts it at once, where Add for later
/// needed Start Queue after it (gaps row U25).
#[test]
fn the_grabbers_start_now_starts_what_it_found() {
    let world = world();
    let mut h = world.harness();
    h.click("Grabber");
    h.until("the Grabber", |h| h.dialog_open("Grabber"));
    h.type_into("Start page", &world.url("page.html"));
    h.type_into("File types", "zip pdf");
    h.click("Find files");
    h.until("the files found", |h| h.shows_part("Files found: 2 "));
    h.click("Start now");
    h.until("both fetched", |h| {
        let rows = h.rows();
        rows.len() == 2 && rows.iter().all(|r| r.state == dm_core::State::Completed)
    });
}

/// A search a page deep reads the pages of the start page's site; saved to run every hour, it
/// runs an hour on and its files are added for later, and run again by hand it adds nothing
/// twice. The egui window also listed it under Grabber projects in the side panel, which the
/// GPUI window has not yet (gaps row U40).
#[test]
fn a_site_search_reads_pages_deep_and_a_saved_one_runs_on_its_own() {
    let world = world();
    let mut h = world.harness();
    h.click("Grabber");
    h.until("the Grabber", |h| h.dialog_open("Grabber"));
    h.type_into("Start page", &world.url("page.html"));
    h.type_into("Page depth (0 for the start page only)", "1");
    h.type_into("File types", "zip pdf");
    h.click("Find files");
    // page.html and next.html on this site; the page elsewhere is not read.
    h.until("the search to go a page deep", |h| {
        h.shows_part("Files found: 3 · pages read: 2")
    });

    // Saved to run every hour: not at once, its first run is an hour from saving.
    h.type_into("Save this search as", "Nightly");
    h.type_into("Run every (hours, 0 for manual)", "1");
    h.click("Save search");
    h.until("the search saved", |h| {
        !h.view(|v| v.backend.settings().site_searches.is_empty())
    });
    let saved = h.view(|v| v.backend.settings().site_searches);
    assert_eq!(saved.len(), 1);
    assert_eq!(
        (saved[0].name.as_str(), saved[0].depth, saved[0].every_hours),
        ("Nightly", 1, 1)
    );
    assert_eq!(saved[0].types, ["zip", "pdf"]);
    h.until_shown("Saved searches");
    assert!(h.rows().is_empty(), "it ran as it was saved");
    // An hour on.
    h.settings_to(|s| s.site_searches[0].last_run -= 3600);
    h.until("the saved search's files", |h| h.rows().len() == 3);
    assert!(h
        .rows()
        .iter()
        .all(|r| r.state == State::Paused && r.queue == dm_core::MAIN_QUEUE));
    assert!(h.view(|v| v.backend.settings().site_searches[0].last_run > 0));

    h.click("Run");
    h.until("the search to come back", |h| {
        h.shows_part("The search Nightly found nothing new.")
    });
    assert_eq!(h.rows().len(), 3, "found twice, added twice");
    // The list emptied: what the search found before is still not new.
    let ids: Vec<_> = h.rows().iter().map(|r| r.id).collect();
    h.view(|v| {
        for id in ids {
            v.backend.remove(id);
        }
    });
    h.until("the list emptied", |h| h.rows().is_empty());
    h.view(|v| v.said = None);
    h.click("Run");
    h.until("the search to come back again", |h| {
        h.shows_part("found nothing new")
    });
    assert!(
        h.rows().is_empty(),
        "found again after the list was cleared"
    );

    h.click("Delete search");
    h.until("the search deleted", |h| {
        h.view(|v| v.backend.settings().site_searches.is_empty())
    });
}

#[test]
fn export_and_import_carry_the_list_as_a_dmlist_file() {
    let world = world();
    let mut h = world.harness();
    h.view(|v| {
        v.backend.add_with(
            &world.url("file.zip"),
            dm_core::AddOptions {
                referrer: "https://page.example/".into(),
                later: true,
                ..Default::default()
            },
        )
    })
    .unwrap();
    h.until("the row", |h| !h.rows().is_empty());
    let file = world.dir.path().join("list.dmlist");
    h.choose_ahead(&file);
    h.command("program.export");
    h.until("the list written", |_| file.is_file());
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("\"referrer\": \"https://page.example/\""),
        "{text}"
    );

    let other = crate::harness::world::world();
    let mut other_h = other.harness();
    other_h.choose_ahead(&file);
    other_h.command("program.import");
    other_h.until("the list imported", |h| h.rows().len() == 1);
    assert_eq!(other_h.rows()[0].referrer, "https://page.example/");
}

#[test]
fn an_address_already_in_the_list_is_asked_about_before_it_is_added_again() {
    let world = world();
    let mut h = world.harness();
    let url = world.url("file.zip");
    h.add_url(&url, "Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let id = h.rows()[0].id;

    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(&url);
    h.click("OK");
    h.until("the question", |h| h.shows("Already in the list"));
    assert!(!h.dialog_open("Download details"));
    // The row is stopped ("Add for later"), so the question offers to go on with it as well as
    // to show it.
    h.click("Resume it");
    h.until("the question to go", |h| !h.shows("Already in the list"));
    assert!(
        h.view(|v| v.selected.contains(&id)),
        "the row was not shown"
    );
    assert_eq!(h.rows().len(), 1);

    // Asked again, and this time wanted again: the usual dialog follows.
    h.add_url(&url, "Add for later");
    h.until("a second row", |h| h.rows().len() == 2);
}

/// Download details' Category sends a file to the folder of the kind picked, here a .zip to
/// Documents rather than Archives.
#[test]
fn download_details_category_picks_the_folder() {
    let world = world();
    // With sorting into kind folders off, as it is unless turned on: the pick is still that
    // kind's folder.
    let mut h = world.harness();
    details(&mut h, vec![world.url("file.zip")]);
    h.click("As its type says");
    h.click("Documents");
    h.until("the Documents folder", |h| {
        h.texts()
            .iter()
            .any(|t| t.ends_with("Documents") && t.contains(std::path::MAIN_SEPARATOR))
    });
    h.click("Start now");
    h.until("it to complete", finished);
    let path = h.rows()[0].path.clone();
    assert!(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .ends_with("Documents"),
        "{path}"
    );
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
    h.settings_to(|s| s.ask_panel_downloads = true);
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
    assert!(h.view(|v| v.backend.settings().catch.skip_hosts.is_empty()));
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
        h.view(|v| v.backend.settings().catch.skip_hosts) == ["127.0.0.1"]
    });
}

#[test]
fn a_handed_over_videos_sign_in_survives_download_details() {
    let world = world();
    let mut h = world.harness();
    h.settings_to(|s| s.ask_panel_downloads = true);
    h.hand(Request {
        cookies: "session=1".into(),
        ..handed(&world, "file.zip")
    });
    h.until("Download details", |h| h.dialog_open("Download details"));
    h.click("Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let rows = h.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].cookies, "session=1",
        "Download details dropped the sign-in"
    );

    // A page's links: its sign-in goes with the links on its own server only.
    h.hand(Request {
        url: world.url("page.html"),
        referrer: world.url("page.html"),
        links: vec![
            world.url("other.pdf"),
            "http://elsewhere.invalid/x.zip".into(),
        ],
        ask: true,
        cookies: "session=2".into(),
        ..Default::default()
    });
    h.until("Download links", |h| h.dialog_open("Download links"));
    // The made-up server cannot be asked, so its link is unticked: ticked again, as a person
    // would who wants it anyway.
    h.until("the made-up server's link to be refused", |h| {
        h.shows("Error")
    });
    h.click("Check all");
    h.click("Add for later");
    h.until("the links added", |h| h.rows().len() == 3);
    let rows = h.rows();
    let of = |end: &str| {
        rows.iter()
            .find(|r| r.url.ends_with(end))
            .map(|r| r.cookies.clone())
    };
    assert_eq!(of("/other.pdf").as_deref(), Some("session=2"));
    assert_eq!(
        of("/x.zip").as_deref(),
        Some(""),
        "another server was given the page's sign-in"
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
            at.elapsed() > crate::clipwatch::EVERY * 2
        });
    };
    clip(&mut h, &world.url("new.iso"));
    looked(&mut h);
    assert!(!h.shows(offer), "offered while off");

    h.click("Options");
    h.until("Options", |h| h.dialog_open("Options"));
    let watch = "Offer to download links I copy";
    // Lower on General than the window shows: scrolled to, as a person scrolls, as far as it
    // takes (Windows' General has more above it).
    h.reveal(watch);
    h.click(watch);
    h.click("OK");
    h.until("the watch on", |h| {
        h.view(|v| v.backend.settings().watch_clipboard)
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
    let mut settings = world.settings();
    settings.show_progress = true;
    let mut h = world.harness_with(settings);
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

/// A folder's field has a Browse… button, and what it picks is where the file goes.
#[test]
fn browse_picks_the_folder_a_download_goes_to() {
    let world = world();
    let mut h = world.harness();
    let picked = folder_in(&world, "picked");
    h.click("Add URL");
    h.until_shown("Address");
    h.type_text(&world.url("file.zip"));
    h.click("OK");
    h.until("Download details", |h| h.dialog_open("Download details"));
    h.click("Browse…");
    h.answer_folder_panel(&picked);
    h.until("the folder picked", |h| {
        h.shows(&picked.display().to_string())
    });
    h.click("Start now");
    h.until("it to finish", finished);
    assert!(std::path::Path::new(&h.rows()[0].path).starts_with(&picked));

    h.click("Options");
    h.until("Options", |h| h.dialog_open("Options"));
    h.click("Folders");
    h.until("Options › Folders", |h| h.shows("Browse…"));
    assert_eq!(h.texts().iter().filter(|t| *t == "Browse…").count(), 2);
}

/// A sign-in saved in Properties on a download refused for want of one tries it again, and it
/// downloads.
#[test]
fn a_sign_in_saved_in_properties_retries_the_download() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("locked.zip"), "Start now");
    h.until("the refusal", |h| {
        h.rows().iter().any(|r| matches!(r.state, State::Failed(_)))
    });
    h.select_by_port("locked.zip");
    h.command("file.properties");
    h.until("File properties", |h| h.dialog_open("File properties"));
    // The keyboard is in Login already.
    h.type_text("user");
    h.until("the login typed", |h| h.shows("user"));
    h.type_into("Password", "pass");
    h.click("OK");
    h.until("Trying it again", |h| {
        h.said().contains("again with the sign-in")
    });
    h.until("it to download with the sign-in", all_finished);
}

/// Import asks, once, about addresses already in the list: Skip them adds only the new ones,
/// Add them again adds them all.
#[test]
fn import_asks_about_addresses_already_listed() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let file = world.dir.path().join("two.txt");
    std::fs::write(
        &file,
        format!("{}\n{}\n", world.url("file.zip"), world.url("other.pdf")),
    )
    .unwrap();
    h.view(|v| v.backend.pause_all());
    h.choose_ahead(&file);
    h.command("program.import");
    h.until_shown("Skip them");
    assert_eq!(h.rows().len(), 1, "imported without asking");
    assert!(h.shows_part("1 of the 2 addresses in this list"));
    h.click("Skip them");
    h.until("only the new one added", |h| h.rows().len() == 2);

    h.choose_ahead(&file);
    h.command("program.import");
    h.until_shown("Add them again");
    h.click("Add them again");
    h.until("both added again", |h| h.rows().len() == 4);
}

/// A fresh link can be got from the browser: its page opens there, and the next download
/// caught becomes the row's link, with no row of its own.
#[test]
fn a_fresh_link_is_taken_from_the_browser() {
    let world = world();
    let mut settings = world.settings();
    settings.attempts = 3;
    settings.retry_wait = 0;
    settings.record_minutes = 0;
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

/// A file of the name already in the folder: Download details offers to replace it, and it is
/// replaced only once the new one is whole; a number is still the default.
#[test]
fn download_details_can_replace_a_file_of_the_same_name() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", finished);
    let first = std::path::PathBuf::from(&h.rows()[0].path);
    std::fs::write(&first, b"an older version").unwrap();
    h.add_url_open(&world.url("file.zip"));
    let replace = "Replace that file once this one is whole";
    h.until_shown(replace);
    assert!(h.shows("Save this one as file (1).zip"));
    h.click(replace);
    h.click("Start now");
    h.until("the second download to complete", |h| {
        h.rows().len() == 2 && all_finished(h)
    });
    assert_eq!(std::fs::read(&first).unwrap(), payload(), "not replaced");
    let dir = first.parent().unwrap();
    assert!(
        !dir.join("file (1).zip").exists(),
        "a numbered copy was left"
    );
    assert!(h
        .rows()
        .iter()
        .all(|r| std::path::Path::new(&r.path) == first));
}

/// Download details says the name a file will really get where its folder has one of that
/// name already, and that the server has no file at an address, before Start now rather than
/// after.
#[test]
fn download_details_says_the_name_it_will_get_and_a_server_refusal() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", finished);
    h.add_url_open(&world.url("file.zip"));
    h.until_shown("Save this one as file (1).zip");
    h.click("Cancel");

    h.add_url_open(&world.url("missing.bin"));
    h.until_shown("Not available: this link no longer works.");

    // The first copy still under way, not yet on disk: said as well.
    h.click("Cancel");
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.add_url_open(&world.url("stalled.bin"));
    h.until_shown("A download with this name exists. This one will be numbered.");
}

/// Behind Options the toolbar and the places take no clicks (B42: Options holds the dm window).
/// The egui window's progress window, drawn inside the dm window, took none either; the GPUI
/// window's is a window of its own beside dm's (B42), which Options does not hold.
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
    let before = h.view(|v| v.place.clone());
    h.click("Finished");
    assert_eq!(
        h.view(|v| v.place.clone()),
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

/// Import of a plain list says how many lines it skipped for having no address dm can read,
/// rather than passing over them.
#[test]
fn import_says_the_lines_it_could_not_read() {
    let world = world();
    let mut h = world.harness();
    let file = world.dir.path().join("plain.txt");
    std::fs::write(
        &file,
        format!(
            "{}\n127.0.0.1:8000/notes.txt\n# a comment\n\n{}\n",
            world.url("other.pdf"),
            world.url("deep.pdf")
        ),
    )
    .unwrap();
    h.choose_ahead(&file);
    h.command("program.import");
    h.until("Import to say", |h| h.said().starts_with("Imported"));
    let said = h.said();
    assert!(said.starts_with("Imported 2. Skipped 1 lines"), "{said}");
}

/// A list that cannot be written is said so, with why: the file panel chose a folder that is
/// not there. (The egui window's dialog for a path typed by hand, where there is no file
/// panel, kept itself open with it; the GPUI window has no such dialog yet, gaps row U41.)
#[test]
fn a_list_that_cannot_be_written_keeps_its_dialog_open() {
    let world = world();
    let mut h = world.harness();
    let path = world.dir.path().join("no such folder").join("list.dmlist");
    h.choose_ahead(&path);
    h.command("program.export");
    h.until("Export to say", |h| {
        h.said().contains("could not be written")
    });
    assert!(
        h.said().contains(&path.display().to_string()),
        "{}",
        h.said()
    );
}

/// Several links copied at once are all offered, through the dialog that lists them; only the
/// first was, and the rest dropped.
#[test]
fn the_clipboard_watcher_offers_every_link_copied_together() {
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some("old".into());
    let mut h = world.harness();
    h.settings_to(|s| s.watch_clipboard = true);
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

/// A long link on the clipboard is cut short, and its Download and Dismiss stay in the window.
#[test]
fn the_clipboard_bar_keeps_its_buttons_in_the_window() {
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some("old".into());
    let mut h = world.harness();
    h.settings_to(|s| s.watch_clipboard = true);
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
    let before = h.view(|v| v.backend.settings().read_timeout);
    let said = "Numbers only: the one before was kept";
    h.type_into(label, "abc");
    assert!(!h.shows(said), "said while still typing");
    // Left for another box (Attempts', which holds 15).
    h.person_click("15", 1);
    h.until("the box to say so", |h| h.shows(said));
    h.click("OK");
    h.until("Options to close", |h| !h.dialog_open("Options"));
    assert_eq!(
        h.view(|v| v.backend.settings().read_timeout),
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
    assert_eq!(h.view(|v| v.backend.settings().on_close), "ask");
    let question = "Keep running in the tray?";
    h.act(|view, cx| view.ask_close(cx));
    h.until_shown(question);
    h.click("Keep running");
    h.until("the window sent to the tray", |h| h.view(|v| v.hidden));
    assert!(!h.shows(question));
    assert_eq!(h.view(|v| v.backend.settings().on_close), "tray");

    // The other answer, from a fresh question.
    h.act(|view, cx| view.show_main(cx));
    h.settings_to(|s| s.on_close = "ask".into());
    h.act(|view, cx| view.ask_close(cx));
    h.until_shown(question);
    h.click("Exit");
    h.until("Exit to quit", |_| crate::question::is_quitting());
    assert_eq!(h.view(|v| v.backend.settings().on_close), "exit");
}

/// The welcome is three steps: the browser, where downloads go, and Done. The folder shown is
/// the one in force, and a change is kept on Done. A folder a script typed is kept only from a
/// person's Done: settings that say where files go are a person's to change (gaps row 199).
#[test]
fn the_welcome_confirms_where_downloads_go_and_ends_with_done() {
    let world = world();
    let mut h = world.harness();
    let was = h.view(|v| v.backend.settings().download_dir);
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
    assert_eq!(h.view(|v| v.backend.settings().download_dir), elsewhere);
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

/// A part can be made vertical or square in Download details: the choice shows once Part is
/// ticked, and goes with the part to its row.
#[test]
fn a_part_can_be_made_vertical_or_square() {
    let world = world();
    let mut h = world.harness();
    details(
        &mut h,
        vec!["https://www.youtube.com/watch?v=jNQXAC9IVRw&t=4".into()],
    );
    h.until_shown("From 0:04");
    assert!(!h.shows("Vertical 9:16"));
    h.click("From 0:04");
    h.until_shown("Vertical 9:16");
    // The part's "to", behind Add for later's "to" its queue.
    h.type_into_nth("to", 1, "0:09");
    h.click("Vertical 9:16");
    h.click("Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let row = h.rows().last().cloned().unwrap();
    assert_eq!(row.shape, Some(dm_core::shape::Shape::Vertical));
    assert!(row.span.is_some());
}

/// A part can be saved as a GIF where dm can make one: the choice shows with the part, and
/// goes with it to its row.
#[cfg(target_os = "macos")]
#[test]
fn a_part_can_be_saved_as_a_gif() {
    let world = world();
    let mut h = world.harness();
    details(
        &mut h,
        vec!["https://www.youtube.com/watch?v=jNQXAC9IVRw&t=4".into()],
    );
    h.until_shown("From 0:04");
    assert!(!h.shows("GIF"));
    h.click("From 0:04");
    h.until_shown("GIF");
    // The part's "to", behind Add for later's "to" its queue.
    h.type_into_nth("to", 1, "0:09");
    h.click("GIF");
    h.click("Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let row = h.rows().last().cloned().unwrap();
    assert!(row.gif, "the row does not carry the GIF");
    assert!(row.span.is_some());
}

/// "Delete file after" is chosen as a download is saved, in Download details, and goes with it
/// to its row.
#[test]
fn delete_file_after_is_chosen_when_saving() {
    let world = world();
    let mut h = world.harness();
    details(&mut h, vec![world.url("file.zip")]);
    named(&mut h, "file.zip");
    h.click("Never");
    h.click("1 hour");
    h.until_shown("1 hour");
    h.click("Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    assert_eq!(h.rows()[0].delete_after, Some(3600));
}

/// The part the browser panel's "Part" line asked for: Download details opens with "Only a
/// part of the video" on and its ends filled in, and a part given only an end starts from the
/// beginning.
#[test]
fn a_part_asked_for_in_the_browsers_panel_fills_download_details() {
    let world = world();
    let mut h = world.harness();
    h.settings_to(|s| s.ask_panel_downloads = true);
    for (from, to, shown_from) in [("1:30", "2:00", "1:30"), ("", "45", "0:00")] {
        h.hand(Request {
            url: world.url("file.zip"),
            referrer: "https://video.example/watch".into(),
            ask: true,
            from: from.into(),
            to: to.into(),
            ..Default::default()
        });
        h.until("Download details", |h| h.dialog_open("Download details"));
        h.until_shown(shown_from);
        assert!(h.shows(to), "{:?}", h.texts());
        assert!(h.shows("Only a part of the video"));
        h.click("Cancel");
        // A second hand-off from the site cancelled asks whether to leave the site to the
        // browser: not this test's.
        h.until("it to close", |h| {
            h.dialogs().is_empty() || h.shows("Keep handing them over")
        });
        if h.shows("Keep handing them over") {
            h.click("Keep handing them over");
            h.until("the question to go", |h| !h.shows("Keep handing them over"));
        }
    }
    // No part asked for: the dialog is as it was, the part's ends not shown.
    h.hand(Request {
        url: world.url("file.zip"),
        ask: true,
        ..Default::default()
    });
    h.until("Download details", |h| h.dialog_open("Download details"));
    named(&mut h, "file.zip");
    assert!(!h.shows("Vertical 9:16"), "the part is on: {:?}", h.texts());
}

/// With Download details off for the panel's videos, a part typed on the "Part" line still
/// opens it, the part filled in, as contract row B59 says: added at once, the part was dropped
/// and the whole video fetched. With nothing typed the video is added at once, as the setting
/// says.
#[test]
fn a_part_asked_for_opens_download_details_where_it_is_off() {
    let world = world();
    let mut h = world.harness();
    let hand = |h: &mut Harness, from: &str, to: &str| {
        h.hand(Request {
            url: world.url("file.zip"),
            referrer: "https://video.example/watch".into(),
            ask: true,
            from: from.into(),
            to: to.into(),
            ..Default::default()
        });
    };
    hand(&mut h, "1:30", "2:00");
    h.until("Download details opened for the part", |h| {
        h.dialog_open("Download details")
    });
    h.until_shown("1:30");
    assert!(h.shows("2:00"));
    assert!(h.rows().is_empty(), "added without asking");
    h.click("Cancel");
    h.until("it to close", |h| h.dialogs().is_empty());
    hand(&mut h, "", "");
    h.until("the video added at once", |h| {
        h.rows().iter().any(|r| r.url.ends_with("/file.zip"))
    });
    assert!(
        !h.dialog_open("Download details"),
        "asked, with nothing typed"
    );
}

/// With "Take a copied link at once" on, a link copied opens its Download details with no bar
/// first, as reference A's switch does; off, it is offered as before.
#[test]
fn a_copied_link_is_taken_at_once_where_that_is_on() {
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some("nothing yet".into());
    let mut h = world.harness();
    h.settings_to(|s| {
        s.watch_clipboard = true;
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
    h.until("a look", |_| at.elapsed() > crate::clipwatch::EVERY * 2);
    clip(&mut h, &copied);
    let at = std::time::Instant::now();
    h.until("a look", |_| at.elapsed() > crate::clipwatch::EVERY * 2);
    assert!(!h.dialog_open("Download details"), "taken twice");
    // Off again: offered in the bar, as before.
    h.settings_to(|s| s.start_copied = false);
    clip(&mut h, &world.url("offered.iso"));
    h.until_shown("Link on your clipboard:");
    assert!(h.shows_part("offered.iso"));
}

/// "Download links" as reference A's: pictures and an address the page gives twice are left
/// out (each can be shown again), Edit opens a link's name and address, and every file can be
/// sent to one folder, a link renamed there keeping its name.
#[test]
fn download_links_hides_pictures_and_repeats_and_saves_where_chosen() {
    let world = world();
    let mut h = world.harness();
    h.act(|_, cx| {
        crate::links::open(
            cx.entity().downgrade(),
            Request {
                url: world.url("page.html"),
                referrer: world.url("page.html"),
                links: vec![
                    world.url("file.zip"),
                    world.url("photo.jpg"),
                    world.url("other.pdf"),
                    world.url("file.zip"),
                ],
                ask: true,
                ..Default::default()
            },
            cx,
        )
    });
    h.until("Download links", |h| h.dialog_open("Download links"));
    let count = |h: &mut Harness, name: &str| h.texts().iter().filter(|t| *t == name).count();
    h.until_shown("other.pdf");
    assert_eq!(count(&mut h, "photo.jpg"), 0, "a picture was listed");
    assert_eq!(count(&mut h, "file.zip"), 1, "a repeat was listed");
    h.click("Hide images");
    h.until("the picture back", |h| h.shows("photo.jpg"));
    h.click("Hide images");
    h.click("Hide repeated files");
    h.until("the repeat back", |h| count(h, "file.zip") == 2);
    assert!(!h.shows("photo.jpg"));
    h.click("Hide repeated files");

    // Edit opens the listed link's name and address: other.pdf's, the last row's, whose Edit
    // is the nearest the front, renamed.
    let edit_other = 0;
    h.call(
        "ui.click",
        serde_json::json!({ "text": "Edit", "nth": edit_other }),
    )
    .unwrap();
    h.until_shown("Done");
    h.type_into("File name", "renamed.pdf");
    h.click("Done");
    h.until("the name given listed", |h| h.shows("renamed.pdf"));

    // Every file to one folder.
    let folder = folder_in(&world, "links");
    let where_to = folder.display().to_string();
    h.click("Browse…");
    h.answer_folder_panel(&folder);
    h.until("the folder chosen", |h| h.shows(&where_to));
    h.click("Check all");
    h.click("Add for later");
    h.until("the rows", |h| !h.rows().is_empty());
    let rows = h.rows();
    assert_eq!(
        rows.len(),
        2,
        "{:?}",
        rows.iter().map(|r| &r.url).collect::<Vec<_>>()
    );
    assert!(
        rows.iter()
            .all(|r| r.save_dir.as_deref() == Some(where_to.as_str())),
        "not every file goes to the folder chosen"
    );
    let pdf = rows.iter().find(|r| r.url.ends_with("/other.pdf")).unwrap();
    assert_eq!(pdf.save_name.as_deref(), Some("renamed.pdf"));
}

/// Each link's words on the page show in a Link text column and become its download's
/// description, as reference A's "Use link texts as descriptions"; unticked, none is kept.
#[test]
fn a_links_words_on_the_page_are_shown_and_kept_as_its_description() {
    let world = world();
    let mut h = world.harness();
    let texts: std::collections::BTreeMap<String, String> = [
        (world.url("file.zip"), "The whole archive".to_string()),
        (world.url("other.pdf"), "The manual".to_string()),
    ]
    .into_iter()
    .collect();
    let request = Request {
        url: world.url("page.html"),
        referrer: world.url("page.html"),
        links: vec![world.url("file.zip"), world.url("other.pdf")],
        texts,
        // The page's `download` attribute names the archive's file (row 123).
        names: [(world.url("file.zip"), "Whole archive.zip".to_string())]
            .into_iter()
            .collect(),
        ask: true,
        ..Default::default()
    };
    h.act(|_, cx| crate::links::open(cx.entity().downgrade(), request, cx));
    h.until("Download links", |h| h.dialog_open("Download links"));
    h.until_shown("Link text");
    assert!(
        h.shows("The whole archive"),
        "the link's words are not shown"
    );
    h.click("Add for later");
    h.until("the rows", |h| h.rows().len() == 2);
    let notes: Vec<String> = h.rows().iter().map(|r| r.description.clone()).collect();
    assert!(
        notes.contains(&"The whole archive".to_string()),
        "{notes:?}"
    );
    assert!(notes.contains(&"The manual".to_string()), "{notes:?}");
    let named = h
        .rows()
        .iter()
        .find(|r| r.url.ends_with("/file.zip"))
        .and_then(|r| r.save_name.clone());
    assert_eq!(named.as_deref(), Some("Whole archive.zip"));

    // Unticked, the words are shown and not kept.
    let again = Request {
        url: world.url("page.html"),
        referrer: world.url("page.html"),
        links: vec![world.url("third.bin")],
        texts: [(world.url("third.bin"), "A third".to_string())]
            .into_iter()
            .collect(),
        ask: true,
        ..Default::default()
    };
    h.act(|_, cx| crate::links::open(cx.entity().downgrade(), again, cx));
    h.until("Download links", |h| h.dialog_open("Download links"));
    h.until_shown("A third");
    h.click("Use link texts as descriptions");
    h.click("Check all");
    h.click("Add for later");
    h.until("the third link added", |h| h.rows().len() == 3);
    let third = h
        .rows()
        .into_iter()
        .find(|r| r.url.ends_with("/third.bin"))
        .unwrap();
    assert_eq!(third.description, "");
}

/// "Add for later" in Download links puts the links in the queue chosen beside it, as
/// reference A asks on closing a batch dialog (gaps row 85); the Main queue until another is
/// chosen.
#[test]
fn download_links_adds_for_later_to_the_queue_chosen() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    let id = h.row("file.zip").id;
    h.view(|v| v.backend.set_queue(id, "Night"));
    h.until("the row to be in the Night queue", |h| {
        h.rows().iter().any(|r| r.queue == "Night")
    });
    let request = Request {
        url: world.url("page.html"),
        referrer: world.url("page.html"),
        links: vec![world.url("other.pdf")],
        ask: true,
        ..Default::default()
    };
    h.act(|_, cx| crate::links::open(cx.entity().downgrade(), request, cx));
    h.until("Download links", |h| h.dialog_open("Download links"));
    h.until_shown("Main queue");
    h.click("Main queue");
    h.click("Night");
    h.until_shown("Night");
    h.click("Add for later");
    h.until("the row added", |h| h.rows().len() == 2);
    assert_eq!(h.row("other.pdf").queue, "Night");
    assert_eq!(h.said(), "Added 1 download to Night.");
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

/// With Options' "Start downloading while Download details is open", a plain file starts at
/// once and Start now places it under the name chosen; Cancel removes what was begun (gaps row
/// 85).
#[test]
fn a_file_starts_while_download_details_asks_and_lands_where_chosen() {
    let world = world();
    let mut settings = world.settings();
    settings.start_while_asking = true;
    let mut h = world.harness_with(settings);
    details(&mut h, vec![world.url("file.zip")]);
    h.until("the file to start before Start now", |h| {
        h.rows().iter().any(|r| r.url.ends_with("/file.zip"))
    });
    named(&mut h, "file.zip");
    h.type_into("Save as", "chosen.zip");
    h.click("Start now");
    h.until("the file to finish under the name chosen", |h| {
        h.rows()
            .iter()
            .any(|r| r.state == State::Completed && r.path.ends_with("chosen.zip"))
    });
    assert_eq!(
        h.rows()
            .iter()
            .filter(|r| r.url.ends_with("/file.zip"))
            .count(),
        1,
        "added once, not again on Start now"
    );

    // Cancel: what was begun goes.
    details(&mut h, vec![world.url("other.pdf")]);
    h.until("the second file to start", |h| {
        h.rows().iter().any(|r| r.url.ends_with("/other.pdf"))
    });
    h.click("Cancel");
    h.until("the begun file to be removed", |h| {
        !h.rows().iter().any(|r| r.url.ends_with("/other.pdf"))
    });
}

/// A stopped torrent's Properties lists its files, a tick and a priority each (gaps row 149):
/// High set on one is kept on the download, and so the queue fetches that file first.
#[test]
fn a_torrents_file_is_given_a_priority_in_properties() {
    let world = world();
    let mut h = world.harness();
    h.view(|v| {
        v.backend.add_with(
            &world.url("file.zip"),
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
    })
    .unwrap();
    h.until("the row", |h| !h.rows().is_empty());
    // The row as a launch reads a paused torrent of three files back.
    let mut torrent: dm_core::Entry = h.rows()[0].clone();
    torrent.state = State::Paused;
    torrent.torrent_files = ["a.bin", "b.bin", "c.bin"]
        .iter()
        .map(|name| dm_core::TorrentFile {
            name: (*name).into(),
            size: 1024,
            wanted: true,
            priority: dm_core::TorrentPriority::Normal,
        })
        .collect::<Vec<_>>()
        .into();
    let stored = world.dir.path().join("torrent-queue.json");
    std::fs::write(&stored, serde_json::to_string(&vec![torrent]).unwrap()).unwrap();
    let mut h = world.harness();
    let queue = h.view(|v| v.backend.queue.clone());
    world.runtime.block_on(queue.persist_to(stored));
    h.until("the torrent", |h| {
        h.rows().first().is_some_and(|r| r.torrent_files.len() == 3)
    });
    let name = dm_shell::app::names::name_of(&h.rows()[0]);
    h.select_by_port(&name);
    h.command("file.properties");
    h.until("File properties", |h| h.dialog_open("File properties"));
    // One pop-up a file, each showing its priority; the list scrolled to the third, whose
    // pop-up is the nearest the front of those on screen, set to High.
    h.until_shown("a.bin (1.0 KB)");
    h.call(
        "ui.scroll",
        serde_json::json!({ "text": "a.bin (1.0 KB)", "lines": 5 }),
    )
    .unwrap();
    h.until_shown("c.bin (1.0 KB)");
    h.click("Normal");
    h.click("High");
    h.until_shown("High");
    h.click("OK");
    h.until("the priority kept", |h| {
        h.rows()[0]
            .torrent_files
            .iter()
            .map(|f| f.priority)
            .collect::<Vec<_>>()
            == [
                dm_core::TorrentPriority::Normal,
                dm_core::TorrentPriority::Normal,
                dm_core::TorrentPriority::High,
            ]
    });
}
