// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A page's links: Download links, the site file finder and the grabber.

use super::*;

#[test]
fn a_pages_links_open_the_download_links_dialog() {
    let world = world();
    let mut h = world.harness();
    h.settings_to(|s| s.general.ask_panel_downloads = true);
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

#[test]
fn the_site_file_finder_finds_the_files_a_page_links_to() {
    let world = world();
    let mut h = world.harness();
    h.command("program.site_files");
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
    h.command("program.site_files");
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
/// twice.
#[test]
fn a_site_search_reads_pages_deep_and_a_saved_one_runs_on_its_own() {
    let world = world();
    let mut h = world.harness();
    h.command("program.site_files");
    h.until("the Grabber", |h| h.dialog_open("Grabber"));
    h.type_into("Start page", &world.url("page.html"));
    h.type_into("Page depth", "1");
    h.type_into("File types", "zip pdf");
    h.click("Find files");
    // page.html and next.html on this site; the page elsewhere is not read.
    h.until("the search to go a page deep", |h| {
        h.shows_part("Files found: 3 · pages read: 2")
    });

    // Saved to run every hour: not at once, its first run is an hour from saving.
    h.type_into("Save this search as", "Nightly");
    h.type_into("Run every (hours)", "1");
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
    h.view(|v| v.status.said = None);
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

/// "Download links" as reference A's: pictures and an address the page gives twice are left
/// out (each can be shown again), Edit opens a link's name and address, and every file can be
/// sent to one folder, a link renamed there keeping its name.
#[test]
fn download_links_hides_pictures_and_repeats_and_saves_where_chosen() {
    let world = world();
    let mut h = world.harness();
    h.act(|_, cx| {
        crate::dialogs::links::open(
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
    h.choose_ahead(&folder);
    h.click("Browse…");
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
    h.act(|_, cx| crate::dialogs::links::open(cx.entity().downgrade(), request, cx));
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
    h.act(|_, cx| crate::dialogs::links::open(cx.entity().downgrade(), again, cx));
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
    h.act(|_, cx| crate::dialogs::links::open(cx.entity().downgrade(), request, cx));
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
