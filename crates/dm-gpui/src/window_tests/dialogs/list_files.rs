// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Export and import of the list, and an address already listed.

use super::*;

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
    h.click("Next");
    h.until("the question", |h| h.shows("Already in the list"));
    assert!(!h.dialog_open("Download details"));
    // The row is stopped ("Add for later"), so the question offers to go on with it as well as
    // to show it.
    h.click("Resume it");
    h.until("the question to go", |h| !h.shows("Already in the list"));
    assert!(
        h.view(|v| v.list.selected.contains(&id)),
        "the row was not shown"
    );
    assert_eq!(h.rows().len(), 1);

    // Asked again, and this time wanted again: the usual dialog follows.
    h.add_url(&url, "Add for later");
    h.until("a second row", |h| h.rows().len() == 2);
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
/// not there. (The dialog for a path typed by hand, where there is no file panel, keeps itself
/// open with it: `import_and_export_take_a_typed_path_without_a_file_chooser`.)
#[test]
fn a_list_that_cannot_be_written_keeps_its_dialog_open() {
    let world = world();
    let mut h = world.harness();
    let path = world.dir.path().join("no such folder").join("list.dmlist");
    h.choose_ahead(&path);
    h.command("program.export");
    h.until("Export to say", |h| {
        h.said().contains("couldn’t be written")
    });
    assert!(
        h.said().contains(&path.display().to_string()),
        "{}",
        h.said()
    );
}

/// Where there is no file chooser, Import and Export ask for the file's path, rather than
/// doing nothing (gaps row U41): a path that is not a full one, or a list that cannot be
/// written or read, is said in the dialog, which stays open with the path to be put right.
#[test]
fn import_and_export_take_a_typed_path_without_a_file_chooser() {
    let world = world();
    let mut h = world.harness();
    *world.fake.no_chooser.lock().unwrap() = true;
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    h.command("program.export");
    h.until("Export’s dialog", |h| h.dialog_open("Export the list"));
    h.until_shown("No file picker here: type the path.");
    let offered = h.dialog(|d: &mut crate::dialogs::listfile::ListFile, _, cx| d.typed(cx));
    assert!(offered.ends_with("downloads.dmlist"), "{offered}");
    let nowhere = world.dir.path().join("no such folder").join("list.dmlist");
    h.type_into("File’s path", &nowhere.display().to_string());
    h.click("Export");
    h.until("the write refused", |h| h.shows_part("couldn’t be written"));
    assert!(
        h.dialog_open("Export the list"),
        "closed with the path lost"
    );
    let file = world.dir.path().join("typed.dmlist");
    h.type_into("File’s path", &file.display().to_string());
    h.click("Export");
    h.until("the list written", |_| file.is_file());
    h.until("Export’s dialog closed", |h| {
        !h.dialog_open("Export the list")
    });

    let other = crate::harness::world::world();
    let mut other_h = other.harness();
    *other.fake.no_chooser.lock().unwrap() = true;
    other_h.command("program.import");
    other_h.until("Import’s dialog", |h| h.dialog_open("Import a list"));
    other_h.type_into("File’s path", "relative.dmlist");
    other_h.click("Import");
    other_h.until("the path refused", |h| h.shows_part("full path"));
    let missing = other.dir.path().join("missing.dmlist");
    other_h.type_into("File’s path", &missing.display().to_string());
    other_h.click("Import");
    other_h.until("the read refused", |h| h.shows_part("couldn’t be read"));
    assert!(
        other_h.dialog_open("Import a list"),
        "closed with the path lost"
    );
    other_h.type_into("File’s path", &file.display().to_string());
    other_h.click("Import");
    other_h.until("the list imported", |h| h.rows().len() == 1);
    other_h.until("Import’s dialog closed", |h| {
        !h.dialog_open("Import a list")
    });
}
