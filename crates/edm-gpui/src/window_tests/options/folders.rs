// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Categories, folders, queues and site rules set in Options, and the folders it refuses.

use super::*;

#[test]
fn a_category_made_in_options_takes_its_files_into_its_own_folder() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Folders");
    h.click("Sort finished files into folders by category");
    // At the foot of the page, scrolled to as a person would.
    h.reveal("Add a category");
    h.click("Add a category");
    h.fill("Category", "Lectures");
    h.fill("Types", "zip");
    h.fill("Sites", "127.0.0.1");
    ok(&mut h);
    let made = kept(&mut h).custom_categories;
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(made[0].extensions, ["zip"]);

    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = h.rows()[0].clone();
    assert!(
        std::path::Path::new(&row.path)
            .parent()
            .unwrap()
            .ends_with("Lectures"),
        "{} is not in the category’s folder",
        row.path
    );
    assert_eq!(std::fs::read(&row.path).unwrap(), payload());
    // The side panel lists it, with its one download.
    h.click("Lectures");
    h.until("the category’s one row", |h| {
        h.view(|view| {
            view.list.place == edm_shell::app::places::Place::Custom("Lectures".into())
                && view.list.shown.len() == 1
        })
    });

    // Deleted, it takes nothing more.
    open_options(&mut h);
    tab(&mut h, "Folders");
    // Under the page's fold, scrolled to as a person would.
    h.reveal("Delete category");
    h.click("Delete category");
    ok(&mut h);
    assert!(kept(&mut h).custom_categories.is_empty());
}

#[test]
fn a_queues_own_file_limit_and_the_parts_folder_are_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Downloads");
    h.reveal("Add a queue limit");
    h.click("Add a queue limit");
    // The queue's own "Files at once", under the one for every download.
    h.fill_after("Queues with their own limit", "Files at once", "2");
    tab(&mut h, "Folders");
    let staged = world.dir.path().join("staging");
    h.fill(
        "Folder for unfinished downloads",
        &staged.display().to_string(),
    );
    ok(&mut h);
    let settings = kept(&mut h);
    assert_eq!(
        settings.downloads.queue_limits,
        [edm_shell::settings::QueueLimit {
            queue: edm_core::MAIN_QUEUE.into(),
            files: 2
        }]
    );
    assert_eq!(settings.parts_dir(), staged);
    // In force on the queue.
    assert_eq!(
        h.view(|view| view.backend.queue.queue_limits()),
        std::collections::HashMap::from([(edm_core::MAIN_QUEUE.to_string(), 2)])
    );
    assert_eq!(in_force(&mut h, &world).parts_dir, staged);
}

#[test]
fn a_bad_category_options_opened_with_does_not_fail_an_unrelated_ok() {
    let _file = settings_file();
    let world = world();
    // As if an older EDM had saved it.
    let mut saved = world.settings();
    saved.custom_categories.push(edm_core::Custom {
        name: "Work/Docs".into(),
        extensions: vec!["pdf".into()],
        ..Default::default()
    });
    let wanted = !saved.general.sound_on_failure;
    let mut h = world.harness_with(saved);
    open_options(&mut h);
    h.reveal("Play a sound when a download fails");
    h.click("Play a sound when a download fails");
    ok(&mut h);
    assert_eq!(kept(&mut h).general.sound_on_failure, wanted);
}

#[test]
fn options_refuses_a_category_that_is_not_one_folder_and_a_relative_download_folder() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    let before = kept(&mut h);
    open_options(&mut h);
    tab(&mut h, "Folders");
    h.click("Sort finished files into folders by category");
    h.reveal("Add a category");
    h.click("Add a category");
    h.fill("Category", "Work/Docs");
    h.fill("Types", "pdf");
    h.click_as_person("OK", 0);
    h.until("the refusal", |h| h.shows_part("can’t be a category"));
    assert_eq!(kept(&mut h).custom_categories, before.custom_categories);

    h.reveal("Delete category");
    h.click("Delete category");
    h.fill("Download folder", "Downloads/EDM");
    h.click_as_person("OK", 0);
    h.until("the refusal", |h| h.shows_part("must be a full path"));
    assert_eq!(
        kept(&mut h).folders.download_dir,
        before.folders.download_dir
    );

    // From the home folder is a full path, and is expanded.
    h.fill("Download folder", "~/edm-test-downloads");
    ok(&mut h);
    assert_eq!(
        kept(&mut h).folders.download_dir,
        edm_shell::settings::full_path("~/edm-test-downloads").unwrap()
    );
}

#[test]
fn options_holds_only_new_or_renamed_folders_to_every_computers_rules() {
    let _file = settings_file();
    let world = world();
    // Saved before by an EDM that took them, and usable on this computer: a kind's folder and
    // a category with a colon in it.
    let mut saved = world.settings();
    saved.folders.categorize = true;
    saved.folders.category_folders[1] = "Audio: 2024".into();
    saved.custom_categories.push(edm_core::Custom {
        name: "Live: 2024".into(),
        extensions: vec!["mkv".into()],
        sites: Vec::new(),
    });
    let before = saved.clone();
    let mut h = world.harness_with(saved);
    // An unrelated change goes through.
    open_options(&mut h);
    tab(&mut h, "Downloads");
    h.fill("Connections per file", "4");
    ok(&mut h);
    let now = kept(&mut h);
    assert_eq!(now.downloads.connections, 4);
    assert_eq!(
        now.folders.category_folders[1], "Audio: 2024",
        "a saved folder kept"
    );
    assert!(now.custom_categories.iter().any(|c| c.name == "Live: 2024"));
    // A kind's folder renamed to one Windows cannot make is refused.
    open_options(&mut h);
    tab(&mut h, "Folders");
    h.fill("Videos", "Clips|new");
    h.click_as_person("OK", 0);
    h.until("the refusal", |h| {
        h.shows_part("because its name is also its folder")
    });
    assert_eq!(
        kept(&mut h).folders.category_folders[0],
        before.folders.category_folders[0]
    );
    // And a new category.
    h.fill("Videos", &before.folders.category_folders[0]);
    h.reveal("Add a category");
    h.click("Add a category");
    // The new category's row, after the saved one's.
    h.fill_after("Delete category", "Category", "Notes?");
    h.fill_after("Delete category", "Types", "txt");
    h.click_as_person("OK", 0);
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    assert!(h.dialog_open("Options"), "a new bad name was taken");
}

/// Options › Advanced: a site's rule gives its downloads a folder, a speed limit, a `User-Agent`
/// and request headers of their own, from the moment they are added by any way in.
#[test]
fn a_site_rule_set_in_options_sends_that_sites_downloads_to_its_folder_at_its_speed() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    // Under the download folder: a folder elsewhere is refused to a download a script adds
    // (`control_api::folder_refused`), and this window is driven as a script drives it.
    let folder = world.dir.path().join("out").join("from-the-site");
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.reveal("Add a site rule");
    h.click("Add a site rule");
    h.fill("Site", "127.0.0.1");
    h.fill("Folder", &folder.display().to_string());
    // The rule's own speed, under the page's of the same name.
    h.fill_after(
        "Sites with their own folder, speed and headers",
        "Speed limit (KB/s, 0 for none)",
        "4096",
    );
    // Its own name and a header for its server (gaps row F1).
    h.fill("User-Agent", "SiteAgent/3");
    h.reveal("Delete site rule");
    h.fill("Request headers", "X-Api-Key: k1 | Accept: */*");
    ok(&mut h);
    let rules = kept(&mut h).advanced.site_rules;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].speed, 4096);
    assert_eq!(rules[0].agent, "SiteAgent/3");

    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the file to complete", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
    let done = h.rows()[0].clone();
    assert_eq!(done.speed_limit, 4096, "its own speed limit, from the rule");
    assert_eq!(
        done.headers,
        [
            ("X-Api-Key", "k1"),
            ("Accept", "*/*"),
            ("User-Agent", "SiteAgent/3")
        ]
        .map(|(n, v)| (n.to_string(), v.to_string())),
        "the rule’s agent and headers, from the rule"
    );
    assert_eq!(done.save_dir.as_deref(), folder.to_str());
    assert!(
        std::path::Path::new(&done.path).starts_with(&folder),
        "it landed in the site’s folder: {}",
        done.path
    );

    // The rule can be deleted again.
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.reveal("Delete site rule");
    h.click("Delete site rule");
    ok(&mut h);
    assert!(kept(&mut h).advanced.site_rules.is_empty());
}

/// Unpack when done (gaps row W14): a kind ticked and "Send the archive to the Trash" chosen
/// in Options › Folders are kept and written down, shown so when Options opens again, and
/// undone there.
#[test]
fn unpack_when_done_is_chosen_for_a_kind_in_options_and_kept() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    assert!(
        kept(&mut h).folders.unpack_kinds.is_empty(),
        "none in a new EDM"
    );
    open_options(&mut h);
    tab(&mut h, "Folders");
    h.reveal("Send the archive to the Trash");
    // The kind's box: the last "Archives" drawn, under the kind's folder of that name.
    h.click("Archives");
    h.click("Send the archive to the Trash");
    ok(&mut h);
    let settings = kept(&mut h);
    assert_eq!(
        settings.folders.unpack_kinds,
        [edm_core::Category::Archives]
    );
    assert!(settings.folders.unpack_trash);
    assert_eq!(
        saved_settings()["unpack_kinds"],
        serde_json::json!(["archives"])
    );
    assert_eq!(saved_settings()["unpack_trash"], true);

    open_options(&mut h);
    tab(&mut h, "Folders");
    let shown = h.dialog(|o: &mut Options, _, _| o.draft.folders.clone());
    assert_eq!(shown.unpack_kinds, [edm_core::Category::Archives]);
    assert!(shown.unpack_trash);
    h.reveal("Keep the archive");
    h.click("Archives");
    h.click("Keep the archive");
    ok(&mut h);
    let settings = kept(&mut h);
    assert!(settings.folders.unpack_kinds.is_empty());
    assert!(!settings.folders.unpack_trash);
}

/// Options › Folders keeps the passwords tried on an archive that has one (gaps row C2): one
/// added, shown only as dots, is kept sealed by OK, not in the settings; deleted, it goes.
#[test]
fn an_archive_password_added_in_options_is_kept_apart_and_shown_as_dots() {
    let _file = settings_file();
    let world = world();
    let home = edm_shell::settings::home();
    let _ = std::fs::remove_file(home.join(edm_core::paths::ARCHIVE_PASSWORDS));
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Folders");
    h.reveal("Add a password");
    h.fill("Add a password", "EDM secret");
    h.click("Add");
    h.until("the password listed as dots", |h| h.shows("••••••••"));
    assert!(!h.shows("EDM secret"), "the password was shown");
    ok(&mut h);
    assert_eq!(edm_core::extract::passwords::load(&home), ["EDM secret"]);
    let settings = std::fs::read_to_string(edm_shell::Settings::path()).unwrap_or_default();
    assert!(
        !settings.contains("EDM secret"),
        "the password is in the settings"
    );
    // Deleted, and OK: it goes.
    open_options(&mut h);
    tab(&mut h, "Folders");
    h.reveal("••••••••");
    h.click("Delete");
    h.until("the password gone", |h| !h.shows("••••••••"));
    ok(&mut h);
    assert!(edm_core::extract::passwords::load(&home).is_empty());
}
